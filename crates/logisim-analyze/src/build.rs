//! Turn the expressions of an [`AnalyzerModel`] into a wired circuit of gates.
//!
//! The circuit has one labelled input pin and one labelled output pin per *bit* of every
//! variable (a bus `D[3..0]` becomes pins `D[3]` … `D[0]`). Gates are shared where two outputs
//! need the same thing, and every signal gets its own vertical wire so the result is easy to
//! read and edit.

use crate::expr::Expr;
use crate::model::AnalyzerModel;
use logisim_format::{CircuitDef, ComponentDef, Error as FormatError, InterfaceDef, WireDef};
use std::collections::HashMap;
use std::fmt;

/// How to build the circuit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOptions {
    /// Name of the new circuit.
    pub name: String,
    /// Give every gate at most two inputs.
    pub two_input_only: bool,
    /// Use NAND gates only (inverters become NANDs with both inputs tied together).
    pub nand_only: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            name: "Analyzed".into(),
            two_input_only: false,
            nand_only: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildError {
    NoInputs,
    NoOutputs,
    Format(FormatError),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::NoInputs => write!(f, "add at least one input variable first"),
            BuildError::NoOutputs => write!(f, "add at least one output variable first"),
            BuildError::Format(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<FormatError> for BuildError {
    fn from(e: FormatError) -> Self {
        BuildError::Format(e)
    }
}

/// Widest gate the Gates library offers.
const MAX_FAN_IN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Not,
    And,
    Or,
    Nand,
    Nor,
    Xor,
    Xnor,
}

impl Kind {
    fn part(self) -> &'static str {
        match self {
            Kind::Not => "NOT Gate",
            Kind::And => "AND Gate",
            Kind::Or => "OR Gate",
            Kind::Nand => "NAND Gate",
            Kind::Nor => "NOR Gate",
            Kind::Xor => "XOR Gate",
            Kind::Xnor => "XNOR Gate",
        }
    }
    /// The non-inverting gate that builds the parts of a wide gate.
    fn base(self) -> Kind {
        match self {
            Kind::Nand => Kind::And,
            Kind::Nor => Kind::Or,
            Kind::Xnor => Kind::Xor,
            k => k,
        }
    }
    fn negated(self) -> Kind {
        match self {
            Kind::And => Kind::Nand,
            Kind::Nand => Kind::And,
            Kind::Or => Kind::Nor,
            Kind::Nor => Kind::Or,
            Kind::Xor => Kind::Xnor,
            Kind::Xnor => Kind::Xor,
            Kind::Not => Kind::Not,
        }
    }
}

/// A gate-level expression tree before sharing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tree {
    Const(bool),
    /// Input bit by column, possibly inverted.
    Input(usize, bool),
    Gate(Kind, Vec<Tree>),
}

fn flatten<'a>(e: &'a Expr, kind: Kind, out: &mut Vec<&'a Expr>) {
    match (kind, e) {
        (Kind::And, Expr::And(a, b))
        | (Kind::Or, Expr::Or(a, b))
        | (Kind::Xor, Expr::Xor(a, b)) => {
            flatten(a, kind, out);
            flatten(b, kind, out);
        }
        _ => out.push(e),
    }
}

fn to_tree(e: &Expr, neg: bool, names: &[String]) -> Tree {
    match e {
        Expr::Const(c) => Tree::Const(*c != neg),
        Expr::Var(n) => Tree::Input(
            names.iter().position(|x| x == n).expect("known variable"),
            neg,
        ),
        Expr::Not(a) => to_tree(a, !neg, names),
        Expr::And(..) | Expr::Or(..) | Expr::Xor(..) => {
            let kind = e.op().map(|o| match o {
                crate::expr::Op::And => Kind::And,
                crate::expr::Op::Or => Kind::Or,
                _ => Kind::Xor,
            });
            let kind = kind.unwrap();
            let mut kids = vec![];
            flatten(e, kind, &mut kids);
            Tree::Gate(
                if neg { kind.negated() } else { kind },
                kids.into_iter().map(|k| to_tree(k, false, names)).collect(),
            )
        }
        Expr::Xnor(a, b) => Tree::Gate(
            if neg { Kind::Xor } else { Kind::Xnor },
            vec![to_tree(a, false, names), to_tree(b, false, names)],
        ),
    }
}

fn negate(t: Tree) -> Tree {
    match t {
        Tree::Const(c) => Tree::Const(!c),
        Tree::Input(i, inv) => Tree::Input(i, !inv),
        Tree::Gate(k, kids) => Tree::Gate(k.negated(), kids),
    }
}

/// Rewrite XOR/XNOR gates with AND/OR/NOT so only NAND-friendly gates remain.
fn expand_xor(t: Tree) -> Tree {
    match t {
        Tree::Gate(k @ (Kind::Xor | Kind::Xnor), kids) => {
            let mut it = kids.into_iter().map(expand_xor);
            let mut acc = it.next().expect("xor has inputs");
            for y in it {
                let x = acc;
                acc = Tree::Gate(
                    Kind::Or,
                    vec![
                        Tree::Gate(Kind::And, vec![x.clone(), negate(y.clone())]),
                        Tree::Gate(Kind::And, vec![negate(x), y]),
                    ],
                );
            }
            if k == Kind::Xnor {
                negate(acc)
            } else {
                acc
            }
        }
        Tree::Gate(k, kids) => Tree::Gate(k, kids.into_iter().map(expand_xor).collect()),
        other => other,
    }
}

/// A wire's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Sig {
    Const(bool),
    Input(usize),
    Gate(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    kind: Kind,
    inputs: Vec<Sig>,
}

struct Netlist {
    nodes: Vec<Node>,
    index: HashMap<(Kind, Vec<Sig>), usize>,
    cap: usize,
}

impl Netlist {
    fn new(two_input_only: bool) -> Self {
        Netlist {
            nodes: vec![],
            index: HashMap::new(),
            cap: if two_input_only { 2 } else { MAX_FAN_IN },
        }
    }

    fn add(&mut self, kind: Kind, inputs: Vec<Sig>) -> Sig {
        let key = (kind, inputs);
        if let Some(i) = self.index.get(&key) {
            return Sig::Gate(*i);
        }
        let i = self.nodes.len();
        self.nodes.push(Node {
            kind,
            inputs: key.1.clone(),
        });
        self.index.insert(key, i);
        Sig::Gate(i)
    }

    fn not(&mut self, s: Sig) -> Sig {
        match s {
            Sig::Const(c) => Sig::Const(!c),
            // a NOT of a NOT is the original signal
            Sig::Gate(i) if self.nodes[i].kind == Kind::Not => self.nodes[i].inputs[0],
            s => self.add(Kind::Not, vec![s]),
        }
    }

    /// Split `sigs` into balanced halves until each group fits the fan-in limit, building the
    /// parts with the base kind.
    fn group(&mut self, base: Kind, sigs: &[Sig]) -> Sig {
        if sigs.len() == 1 {
            return sigs[0];
        }
        if sigs.len() <= self.cap {
            return self.add(base, sigs.to_vec());
        }
        let split = sigs.len().div_ceil(2);
        let a = self.group(base, &sigs[..split]);
        let b = self.group(base, &sigs[split..]);
        self.add(base, vec![a, b])
    }

    fn gate(&mut self, kind: Kind, sigs: Vec<Sig>) -> Sig {
        if sigs.len() <= self.cap {
            return self.add(kind, sigs);
        }
        let split = sigs.len().div_ceil(2);
        let a = self.group(kind.base(), &sigs[..split]);
        let b = self.group(kind.base(), &sigs[split..]);
        self.add(kind, vec![a, b])
    }

    fn lower(&mut self, t: &Tree) -> Sig {
        match t {
            Tree::Const(c) => Sig::Const(*c),
            Tree::Input(i, false) => Sig::Input(*i),
            Tree::Input(i, true) => self.not(Sig::Input(*i)),
            Tree::Gate(k, kids) => {
                let sigs: Vec<Sig> = kids.iter().map(|k| self.lower(k)).collect();
                self.gate(*k, sigs)
            }
        }
    }

    // ----- NAND-only construction -----

    fn nand_not(&mut self, s: Sig) -> Sig {
        match s {
            Sig::Const(c) => Sig::Const(!c),
            Sig::Gate(i)
                if self.nodes[i].kind == Kind::Nand
                    && self.nodes[i].inputs.len() == 2
                    && self.nodes[i].inputs[0] == self.nodes[i].inputs[1] =>
            {
                self.nodes[i].inputs[0]
            }
            s => self.add(Kind::Nand, vec![s, s]),
        }
    }

    /// NAND of any number of signals, respecting the fan-in limit.
    fn nand_of(&mut self, sigs: &[Sig]) -> Sig {
        if sigs.len() == 1 {
            return self.nand_not(sigs[0]);
        }
        if sigs.len() <= self.cap {
            return self.add(Kind::Nand, sigs.to_vec());
        }
        let split = sigs.len().div_ceil(2);
        let a = self.and_of(&sigs[..split]);
        let b = self.and_of(&sigs[split..]);
        self.add(Kind::Nand, vec![a, b])
    }

    fn and_of(&mut self, sigs: &[Sig]) -> Sig {
        if sigs.len() == 1 {
            sigs[0]
        } else {
            let n = self.nand_of(sigs);
            self.nand_not(n)
        }
    }

    /// The signal carrying the tree's value (`positive`) or its inverse, using NANDs only.
    fn nand_sig(&mut self, t: &Tree, positive: bool) -> Sig {
        let flip = |s: &mut Self, base: Sig, base_is_positive: bool| {
            if base_is_positive == positive {
                base
            } else {
                s.nand_not(base)
            }
        };
        match t {
            Tree::Const(c) => Sig::Const(*c == positive),
            Tree::Input(i, inv) => flip(self, Sig::Input(*i), !*inv),
            Tree::Gate(kind, kids) => {
                // `inner` = the NAND of the kids, which equals the gate's value inverted (And,
                // Nor-of-negations ...) or not, as listed
                let (use_positive_kids, nand_is_positive) = match kind {
                    Kind::And => (true, false),
                    Kind::Nand => (true, true),
                    Kind::Or => (false, true),
                    Kind::Nor => (false, false),
                    _ => unreachable!("XOR gates are expanded before NAND conversion"),
                };
                let sigs: Vec<Sig> = kids
                    .iter()
                    .map(|k| self.nand_sig(k, use_positive_kids))
                    .collect();
                let nand = self.nand_of(&sigs);
                flip(self, nand, nand_is_positive)
            }
        }
    }
}

// ---------- layout ----------

const GRID: i32 = 10;
const PIN_X: i32 = 60;
const PIN_TOP: i32 = 50;
const PIN_STEP: i32 = 40;
const RAIL_STEP: i32 = 20;
const COLUMN: i32 = 140;
const BAND_MARGIN: i32 = 30;

struct Placed {
    at: (i32, i32),
    ports: Vec<(i32, i32)>,
}

fn gate_def(kind: Kind, inputs: usize, at: (i32, i32)) -> Result<ComponentDef, FormatError> {
    let mut d = ComponentDef::new("Gates", kind.part(), at.0, at.1)?;
    if kind != Kind::Not {
        d = d.option("inputs", inputs as i64);
    }
    Ok(d)
}

fn snap(v: i32) -> i32 {
    (v + GRID - 1).div_euclid(GRID) * GRID
}

/// Build the circuit for every output bit of the model.
pub fn build_circuit(model: &AnalyzerModel, opts: &BuildOptions) -> Result<CircuitDef, BuildError> {
    let in_bits = model.input_bits();
    let out_bits = model.output_bits();
    if in_bits.is_empty() {
        return Err(BuildError::NoInputs);
    }
    if out_bits.is_empty() {
        return Err(BuildError::NoOutputs);
    }

    // 1. expressions -> shared gate netlist
    let mut net = Netlist::new(opts.two_input_only);
    let mut out_sigs = vec![];
    for (i, _) in out_bits.iter().enumerate() {
        let e = model.expression(i).simplify();
        let mut tree = to_tree(&e, false, &in_bits);
        let s = if opts.nand_only {
            tree = expand_xor(tree);
            net.nand_sig(&tree, true)
        } else {
            net.lower(&tree)
        };
        out_sigs.push(s);
    }

    // 2. geometry
    let n_in = in_bits.len() as i32;
    let rail_x_input = |i: usize| PIN_X + 60 + RAIL_STEP * i as i32;
    let last_rail = rail_x_input(in_bits.len() - 1);
    let first_gate_x = snap(last_rail + COLUMN);
    let pin_y = |i: usize| PIN_TOP + PIN_STEP * i as i32;
    let mut cursor = PIN_TOP + PIN_STEP * (n_in - 1) + 60;

    let mut placed: Vec<Placed> = vec![];
    for (k, node) in net.nodes.iter().enumerate() {
        let at0 = gate_def(node.kind, node.inputs.len(), (0, 0))?;
        let offs: Vec<(i32, i32)> = (0..=node.inputs.len())
            .map(|p| at0.port_location(p).map(|l| (l.x, l.y)))
            .collect::<Result<_, _>>()?;
        let top = offs.iter().map(|o| o.1).min().unwrap();
        let bottom = offs.iter().map(|o| o.1).max().unwrap();
        let x = first_gate_x + COLUMN * k as i32;
        let y = snap(cursor + BAND_MARGIN - top);
        cursor = y + bottom + BAND_MARGIN;
        placed.push(Placed {
            at: (x, y),
            ports: offs.iter().map(|o| (x + o.0, y + o.1)).collect(),
        });
    }
    let last_gate_x = first_gate_x + COLUMN * net.nodes.len().saturating_sub(1) as i32;
    let out_x = snap(last_gate_x + 2 * COLUMN);
    let out_rows: Vec<i32> = (0..out_bits.len())
        .map(|j| snap(cursor + BAND_MARGIN) + PIN_STEP * j as i32)
        .collect();

    let rail_x = |s: Sig| -> Option<i32> {
        match s {
            Sig::Input(i) => Some(rail_x_input(i)),
            Sig::Gate(k) => Some(placed[k].at.0 + 20),
            Sig::Const(_) => None,
        }
    };
    let source_y = |s: Sig| -> i32 {
        match s {
            Sig::Input(i) => pin_y(i),
            Sig::Gate(k) => placed[k].at.1,
            Sig::Const(_) => unreachable!(),
        }
    };

    // 3. components and wires
    let mut components = vec![];
    let mut wires: Vec<WireDef> = vec![];
    // the lowest point each rail must reach
    let mut rail_end: HashMap<Sig, i32> = HashMap::new();
    let mut wire = |a: (i32, i32), b: (i32, i32)| {
        if a != b {
            wires.push(WireDef { a, b });
        }
    };

    for (i, name) in in_bits.iter().enumerate() {
        components.push(
            ComponentDef::new("Wiring", "Pin", PIN_X, pin_y(i))?.option("label", name.as_str()),
        );
    }
    for (k, node) in net.nodes.iter().enumerate() {
        let p = &placed[k];
        components.push(gate_def(node.kind, node.inputs.len(), p.at)?);
        for (port, s) in node.inputs.iter().enumerate() {
            let rx = rail_x(*s).expect("gate inputs are never constant");
            let (px, py) = p.ports[port + 1];
            wire((rx, py), (px, py));
            let e = rail_end.entry(*s).or_insert(py);
            *e = (*e).max(py);
        }
    }
    for (j, s) in out_sigs.iter().enumerate() {
        let y = out_rows[j];
        components.push(
            ComponentDef::new("Wiring", "Pin", out_x, y)?
                .option("label", out_bits[j].as_str())
                .option("output", true),
        );
        match s {
            Sig::Const(c) => {
                components.push(
                    ComponentDef::new("Wiring", "Constant", out_x - 60, y)?
                        .option("value", *c as i64),
                );
                wire((out_x - 60, y), (out_x, y));
            }
            s => {
                let rx = rail_x(*s).unwrap();
                wire((rx, y), (out_x, y));
                let e = rail_end.entry(*s).or_insert(y);
                *e = (*e).max(y);
            }
        }
    }
    // rails: from the producer down to the last consumer
    for (i, _) in in_bits.iter().enumerate() {
        let s = Sig::Input(i);
        if rail_end.contains_key(&s) {
            wire((PIN_X, pin_y(i)), (rail_x_input(i), pin_y(i)));
        }
    }
    for (k, p) in placed.iter().enumerate() {
        wire(p.ports[0], (p.at.0 + 20, p.at.1));
        let _ = k;
    }
    let mut rails: Vec<(Sig, i32)> = rail_end.into_iter().collect();
    rails.sort();
    for (s, end) in rails {
        let rx = rail_x(s).unwrap();
        wire((rx, source_y(s)), (rx, end));
    }

    let mut def = CircuitDef {
        name: opts.name.clone(),
        interface: vec![],
        components,
        wires,
    };
    def.interface = interface_of(&def);
    Ok(def)
}

/// Subcircuit interface from the labelled pins: inputs on the left, outputs on the right, each
/// side ordered from top to bottom.
fn interface_of(c: &CircuitDef) -> Vec<InterfaceDef> {
    let mut pins: Vec<(&ComponentDef, bool, String)> = c
        .components
        .iter()
        .filter(|d| d.library == "Wiring" && d.part == "Pin")
        .filter_map(|d| {
            let label = d.options.string("label").ok().flatten()?.trim().to_string();
            (!label.is_empty()).then(|| {
                let out = d.options.boolean("output").ok().flatten().unwrap_or(false);
                (d, out, label)
            })
        })
        .collect();
    pins.sort_by_key(|(d, _, _)| (d.at.1, d.at.0));
    let mut out = vec![];
    for want_output in [false, true] {
        for (i, (d, _, label)) in pins.iter().filter(|p| p.1 == want_output).enumerate() {
            out.push(InterfaceDef {
                name: label.clone(),
                input: !want_output,
                width: 1,
                at: d.at,
                offset: (if want_output { 60 } else { 0 }, 20 * i as i32),
            });
        }
    }
    out
}

/// Gate-count summary for display ("3 AND, 1 OR, 2 NOT").
pub fn gate_summary(def: &CircuitDef) -> String {
    let mut counts: Vec<(String, usize)> = vec![];
    for c in def.components.iter().filter(|c| c.library == "Gates") {
        let short = c.part.trim_end_matches(" Gate").to_string();
        match counts.iter_mut().find(|(n, _)| *n == short) {
            Some(e) => e.1 += 1,
            None => counts.push((short, 1)),
        }
    }
    if counts.is_empty() {
        return "no gates".into();
    }
    counts
        .into_iter()
        .map(|(n, c)| format!("{c} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Entry;
    use crate::var::Var;
    use logisim_core::Location;
    use logisim_format::ProjectFile;

    fn model(ins: &[(&str, u32)], outs: &[(&str, u32)]) -> AnalyzerModel {
        let mut m = AnalyzerModel::new();
        for (n, w) in ins {
            m.add_input(Var::new(n, *w)).unwrap();
        }
        for (n, w) in outs {
            m.add_output(Var::new(n, *w)).unwrap();
        }
        m
    }

    /// Run the generated circuit on one input row; one value per output pin (None = not 0/1).
    fn simulate(def: &CircuitDef, row: usize) -> Vec<Option<bool>> {
        let mut def = def.clone();
        let n_in = def
            .components
            .iter()
            .filter(|c| c.part == "Pin" && c.options.boolean("output") != Ok(Some(true)))
            .count();
        // input pins come first, in column order
        for (i, c) in def.components.iter_mut().take(n_in).enumerate() {
            let bit = row >> (n_in - 1 - i) & 1;
            c.options.set("value", bit as i64);
        }
        let mut p = ProjectFile::new("t", &def.name);
        let outs: Vec<(i32, i32)> = def
            .components
            .iter()
            .filter(|c| c.part == "Pin" && c.options.boolean("output") == Ok(Some(true)))
            .map(|c| c.at)
            .collect();
        p.circuits.push(def);
        let mut c = p.instantiate_main().unwrap();
        c.reset();
        assert!(!c.is_oscillating());
        outs.iter()
            .map(|(x, y)| c.value_at(Location::new(*x, *y)).to_u64().map(|v| v == 1))
            .collect()
    }

    /// The circuit must agree with the model's table on every row that is 0 or 1.
    fn verify(m: &AnalyzerModel, opts: &BuildOptions) -> CircuitDef {
        let def = build_circuit(m, opts).unwrap();
        let t = m.table();
        for row in 0..t.row_count() {
            let got = simulate(&def, row);
            for (o, g) in got.iter().enumerate() {
                match t.get(row, o) {
                    Entry::One => assert_eq!(*g, Some(true), "row {row} out {o} {opts:?}"),
                    Entry::Zero => assert_eq!(*g, Some(false), "row {row} out {o} {opts:?}"),
                    _ => assert!(g.is_some(), "row {row} out {o} floating {opts:?}"),
                }
            }
        }
        def
    }

    fn all_options() -> Vec<BuildOptions> {
        let mut v = vec![];
        for two in [false, true] {
            for nand in [false, true] {
                v.push(BuildOptions {
                    name: "T".into(),
                    two_input_only: two,
                    nand_only: nand,
                });
            }
        }
        v
    }

    fn with_expr(ins: &[(&str, u32)], text: &str) -> AnalyzerModel {
        let mut m = model(ins, &[("y", 1)]);
        m.set_expression_text("y", text).unwrap();
        m
    }

    #[test]
    fn simple_gate_circuits_simulate_correctly() {
        for text in [
            "a b",
            "a + b",
            "a ^ b",
            "a = b",
            "~a",
            "~(a b)",
            "~(a + b)",
            "a b + ~a c",
            "(a + b)(~b + c)",
            "a ^ b ^ c",
            "~(a ^ b ^ c)",
            "a + b + c + a",
            "~~a",
            "a b c",
            "(a ^ b) = c",
        ] {
            let m = with_expr(&[("a", 1), ("b", 1), ("c", 1)], text);
            for o in all_options() {
                verify(&m, &o);
            }
        }
    }

    #[test]
    fn every_three_input_function_builds_a_correct_circuit() {
        for bits in 0..256u32 {
            let mut m = model(&[("a", 1), ("b", 1), ("c", 1)], &[("y", 1)]);
            let mut t = m.table().clone();
            for r in 0..8 {
                t.set(r, 0, Entry::from_bool(bits >> r & 1 == 1));
            }
            m.set_table(t);
            verify(&m, &BuildOptions::default());
        }
    }

    #[test]
    fn nand_only_and_two_input_restrictions_hold() {
        let m = with_expr(
            &[("a", 1), ("b", 1), ("c", 1), ("d", 1)],
            "a b c d + ~a ~b ~c + a ^ d",
        );
        for o in all_options() {
            let def = verify(&m, &o);
            for g in def.components.iter().filter(|c| c.library == "Gates") {
                if o.nand_only {
                    assert_eq!(g.part, "NAND Gate", "{o:?}");
                }
                if o.two_input_only {
                    let n = g.options.int("inputs", 1, 64).unwrap().unwrap_or(1);
                    assert!(n <= 2, "{} has {n} inputs", g.part);
                }
            }
        }
        // without restrictions a four-way AND is a single gate
        let plain = build_circuit(
            &with_expr(&[("a", 1), ("b", 1), ("c", 1), ("d", 1)], "a b c d"),
            &BuildOptions::default(),
        )
        .unwrap();
        let gates: Vec<_> = plain
            .components
            .iter()
            .filter(|c| c.library == "Gates")
            .collect();
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].options.int("inputs", 1, 64), Ok(Some(4)));
    }

    #[test]
    fn minimal_expression_is_used_when_none_was_typed() {
        let mut m = model(&[("a", 1), ("b", 1)], &[("y", 1)]);
        let mut t = m.table().clone();
        t.fill_column(0, Entry::Zero);
        t.set(3, 0, Entry::One);
        m.set_table(t);
        let def = verify(&m, &BuildOptions::default());
        assert_eq!(gate_summary(&def), "1 AND");
    }

    #[test]
    fn gates_are_shared_between_outputs() {
        let mut m = model(&[("a", 1), ("b", 1)], &[("x", 1), ("y", 1)]);
        m.set_expression_text("x", "a b").unwrap();
        m.set_expression_text("y", "a b").unwrap();
        let def = verify(&m, &BuildOptions::default());
        assert_eq!(gate_summary(&def), "1 AND");
        let mut m2 = model(&[("a", 1), ("b", 1)], &[("x", 1), ("y", 1)]);
        m2.set_expression_text("x", "~a b").unwrap();
        m2.set_expression_text("y", "~a + b").unwrap();
        let def = verify(&m2, &BuildOptions::default());
        assert_eq!(gate_summary(&def), "1 NOT, 1 AND, 1 OR");
    }

    #[test]
    fn constant_and_pass_through_outputs() {
        let mut m = model(
            &[("a", 1)],
            &[("one", 1), ("zero", 1), ("copy", 1), ("inv", 1)],
        );
        m.set_expression_text("one", "a + ~a").unwrap();
        m.set_expression_text("zero", "a ~a").unwrap();
        m.set_expression_text("copy", "a").unwrap();
        m.set_expression_text("inv", "~a").unwrap();
        for o in all_options() {
            let def = verify(&m, &o);
            let consts = def
                .components
                .iter()
                .filter(|c| c.part == "Constant")
                .count();
            assert_eq!(
                consts, 0,
                "constants fold only when the expression simplifies: {def:?}"
            );
        }
    }

    #[test]
    fn folded_constant_expressions_use_a_constant_part() {
        let mut m = model(&[("a", 1)], &[("one", 1), ("zero", 1)]);
        m.set_expression("one", Expr::Const(true)).unwrap();
        m.set_expression("zero", Expr::and(Expr::var("a"), Expr::Const(false)))
            .unwrap();
        for o in all_options() {
            let def = verify(&m, &o);
            assert_eq!(
                def.components
                    .iter()
                    .filter(|c| c.part == "Constant")
                    .count(),
                2
            );
            assert_eq!(gate_summary(&def), "no gates");
        }
    }

    #[test]
    fn buses_become_one_pin_per_bit() {
        let mut m = model(&[("A", 2), ("b", 1)], &[("Q", 2)]);
        m.set_expression_text("Q[1]", "A[1] b").unwrap();
        m.set_expression_text("Q[0]", "A[0] ^ b").unwrap();
        let def = verify(&m, &BuildOptions::default());
        let labels: Vec<String> = def
            .components
            .iter()
            .filter(|c| c.part == "Pin")
            .map(|c| c.options.string("label").unwrap().unwrap().to_string())
            .collect();
        assert_eq!(labels, ["A[1]", "A[0]", "b", "Q[1]", "Q[0]"]);
        let ifc: Vec<(&str, bool)> = def
            .interface
            .iter()
            .map(|i| (i.name.as_str(), i.input))
            .collect();
        assert_eq!(
            ifc,
            [
                ("A[1]", true),
                ("A[0]", true),
                ("b", true),
                ("Q[1]", false),
                ("Q[0]", false)
            ]
        );
    }

    #[test]
    fn five_input_circuit_with_many_gates() {
        let m = with_expr(
            &[("a", 1), ("b", 1), ("c", 1), ("d", 1), ("e", 1)],
            "a b ~c + ~a d e + b c ~e + (a ^ b ^ c ^ d ^ e) + ~(d + e) a",
        );
        for o in all_options() {
            verify(&m, &o);
        }
    }

    #[test]
    fn layout_is_on_the_grid_and_wires_are_straight() {
        let m = with_expr(&[("a", 1), ("b", 1), ("c", 1)], "a b + ~a c + b ~c");
        let def = build_circuit(&m, &BuildOptions::default()).unwrap();
        for c in &def.components {
            assert!(
                c.at.0 % 10 == 0 && c.at.1 % 10 == 0,
                "{} at {:?}",
                c.part,
                c.at
            );
        }
        for w in &def.wires {
            assert!(w.a.0 == w.b.0 || w.a.1 == w.b.1, "{w:?}");
            assert_ne!(w.a, w.b);
            assert!((w.a.0 % 10, w.a.1 % 10, w.b.0 % 10, w.b.1 % 10) == (0, 0, 0, 0));
        }
        // no two components share a location
        let mut seen = std::collections::HashSet::new();
        for c in &def.components {
            assert!(seen.insert(c.at), "two parts at {:?}", c.at);
        }
    }

    #[test]
    fn project_round_trips_through_the_file_format() {
        let m = with_expr(&[("a", 1), ("b", 1)], "a ^ b");
        let def = build_circuit(&m, &BuildOptions::default()).unwrap();
        let mut p = ProjectFile::new("analysis", &def.name);
        p.circuits.push(def);
        let text = p.to_ron().unwrap();
        assert_eq!(ProjectFile::parse(&text).unwrap(), p);
    }

    #[test]
    fn needs_inputs_and_outputs() {
        let m = AnalyzerModel::new();
        assert_eq!(
            build_circuit(&m, &BuildOptions::default()),
            Err(BuildError::NoInputs)
        );
        let mut m = AnalyzerModel::new();
        m.add_input(Var::new("a", 1)).unwrap();
        assert_eq!(
            build_circuit(&m, &BuildOptions::default()),
            Err(BuildError::NoOutputs)
        );
    }
}
