//! A [`Rig`] that runs test vectors against a circuit of a loaded project.
//!
//! Columns are bound to the circuit's parts by name: a column named `x` is bound to the `Pin`
//! labelled `x` (input pins are driven, output pins are compared). If no pin has that label, a
//! column named `clk` (or `<clk>`, any case) is bound to the circuit's clocks instead.

use logisim_core::{BitWidth, Circuit, CompId, OptionMap, Value};
use logisim_format::{CircuitDef, ComponentDef, ProjectFile};

use crate::testvec::{Rig, TestError, TestVector};

const WIRING: &str = "Wiring";
const PIN: &str = "Pin";
const CLOCK: &str = "Clock";

/// What a test-vector column is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    InputPin,
    OutputPin,
    Clock,
}

#[derive(Debug, Clone, Copy)]
struct Binding {
    comp: CompId,
    kind: ColumnKind,
}

/// A simulated copy of one circuit, with test-vector columns bound to its pins and clocks.
pub struct CircuitRig<'p> {
    project: &'p ProjectFile,
    circuit_name: String,
    circuit: Circuit,
    bindings: Vec<Binding>,
    /// Values set by [`Rig::drive`] that the next [`Rig::propagate`] applies.
    staged: Vec<Option<Value>>,
}

fn is_part(c: &ComponentDef, part: &str) -> bool {
    c.library == WIRING && c.part == part
}

fn is_clock_column(name: &str) -> bool {
    let n = name.trim_matches(|c| c == '<' || c == '>');
    n.eq_ignore_ascii_case("clk")
}

fn bind(def: &CircuitDef, vector: &TestVector) -> Result<Vec<Binding>, TestError> {
    let label_of = |c: &ComponentDef| {
        c.options
            .string("label")
            .ok()
            .flatten()
            .unwrap_or("")
            .to_string()
    };
    let mut out = Vec::with_capacity(vector.column_name.len());
    for (col, name) in vector.column_name.iter().enumerate() {
        let width = vector.column_width[col];
        let pin = def
            .components
            .iter()
            .enumerate()
            .find(|(_, c)| is_part(c, PIN) && label_of(c) == *name);
        if let Some((i, c)) = pin {
            let pin_width = c.options.int("width", 1, 64).ok().flatten().unwrap_or(1) as u8;
            if BitWidth::new(pin_width) != width {
                return Err(TestError(format!(
                    "Column {name} has width {} but pin has width {pin_width}",
                    width.get()
                )));
            }
            let output = c.options.boolean("output").ok().flatten().unwrap_or(false);
            let kind = if output {
                ColumnKind::OutputPin
            } else {
                ColumnKind::InputPin
            };
            out.push(Binding {
                comp: CompId(i),
                kind,
            });
            continue;
        }
        if is_clock_column(name) {
            if let Some(i) = def.components.iter().position(|c| is_part(c, CLOCK)) {
                if width != BitWidth::ONE {
                    return Err(TestError(format!(
                        "Column {name} has width {} but clock has width 1",
                        width.get()
                    )));
                }
                out.push(Binding {
                    comp: CompId(i),
                    kind: ColumnKind::Clock,
                });
                continue;
            }
        }
        return Err(TestError(format!("Column {name} has no matching pin")));
    }
    Ok(out)
}

impl<'p> CircuitRig<'p> {
    /// Build a fresh copy of circuit `circuit_name` and bind every column of `vector`.
    pub fn new(
        project: &'p ProjectFile,
        circuit_name: &str,
        vector: &TestVector,
    ) -> Result<Self, TestError> {
        let def = project
            .circuit(circuit_name)
            .ok_or_else(|| TestError(format!("no circuit named '{circuit_name}'")))?;
        let bindings = bind(def, vector)?;
        let circuit = project
            .instantiate(circuit_name)
            .map_err(|e| TestError(e.to_string()))?;
        let staged = vec![None; bindings.len()];
        let mut rig = CircuitRig {
            project,
            circuit_name: circuit_name.into(),
            circuit,
            bindings,
            staged,
        };
        rig.circuit.reset();
        Ok(rig)
    }

    /// What column `col` is attached to.
    pub fn kind(&self, col: usize) -> ColumnKind {
        self.bindings[col].kind
    }

    /// The simulated circuit (for display).
    pub fn circuit(&self) -> &Circuit {
        &self.circuit
    }
}

impl Rig for CircuitRig<'_> {
    fn reset(&mut self) {
        // Rebuilding restores pin values and clock phases, which `Circuit::reset` leaves alone.
        if let Ok(c) = self.project.instantiate(&self.circuit_name) {
            self.circuit = c;
        }
        self.circuit.reset();
        self.staged.iter_mut().for_each(|s| *s = None);
    }

    fn toggle_clocks(&mut self) {
        self.circuit.tick();
    }

    /// Unknown and error bits of a driven value are applied as 0: pins hold plain numbers.
    fn propagate(&mut self) {
        let mut changed = vec![];
        for (b, staged) in self.bindings.iter().zip(self.staged.iter_mut()) {
            if let (ColumnKind::InputPin, Some(v)) = (b.kind, staged.take()) {
                let n = v.to_u64().unwrap_or(0);
                let opts = OptionMap::new().with("value", n as i64);
                // The option set is always valid for pins; width is unchanged.
                if self
                    .circuit
                    .component_mut(b.comp)
                    .set_options(&opts)
                    .is_ok()
                {
                    changed.push(b.comp);
                }
            }
        }
        self.circuit.refresh_many(&changed);
    }

    fn is_oscillating(&self) -> bool {
        self.circuit.is_oscillating()
    }

    fn is_input(&self, col: usize) -> bool {
        self.bindings[col].kind == ColumnKind::InputPin
    }

    fn is_clock(&self, col: usize) -> bool {
        self.bindings[col].kind == ColumnKind::Clock
    }

    fn read(&mut self, col: usize) -> Value {
        self.circuit.port_value(self.bindings[col].comp, 0)
    }

    fn drive(&mut self, col: usize, v: Value) {
        self.staged[col] = Some(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testvec::TestVectorEvaluator;

    fn and_project() -> ProjectFile {
        let gate = ComponentDef::new("Gates", "AND Gate", 100, 100).unwrap();
        let at = |i: usize| gate.port_location(i).unwrap();
        let pin = |p: logisim_core::Location, label: &str, output: bool| {
            ComponentDef::new("Wiring", "Pin", p.x, p.y)
                .unwrap()
                .option("label", label)
                .option("output", output)
        };
        let mut file = ProjectFile::new("t", "main");
        file.circuits.push(CircuitDef {
            name: "main".into(),
            interface: vec![],
            components: vec![
                pin(at(1), "a", false),
                pin(at(2), "b", false),
                pin(at(0), "q", true),
                gate,
            ],
            wires: vec![],
        });
        file
    }

    fn run(project: &ProjectFile, text: &str) -> Result<(usize, usize), TestError> {
        let v = TestVector::parse(text)?;
        let mut rig = CircuitRig::new(project, "main", &v)?;
        let ev = TestVectorEvaluator::new(&v);
        Ok(ev.evaluate(&mut rig, |_, _| true))
    }

    #[test]
    fn and_gate_vector_passes() {
        let p = and_project();
        assert_eq!(
            run(&p, "a b q\n0 0 0\n0 1 0\n1 0 0\n1 1 1\n").unwrap(),
            (4, 0)
        );
    }

    #[test]
    fn wrong_expectation_fails() {
        let p = and_project();
        assert_eq!(run(&p, "a b q\n1 1 0\n").unwrap(), (0, 1));
    }

    #[test]
    fn unmatched_and_mismatched_columns_are_reported() {
        let p = and_project();
        let e = run(&p, "a x q\n0 0 0\n").err().unwrap();
        assert_eq!(e.0, "Column x has no matching pin");
        let e = run(&p, "a[4] b q\n0 0 0\n").err().unwrap();
        assert_eq!(e.0, "Column a has width 4 but pin has width 1");
    }
}
