//! Work out a truth table by running an existing circuit on every input combination.
//!
//! The circuit's labelled input pins become the inputs and its labelled output pins the outputs,
//! ordered top to bottom as they appear on the sheet.

use crate::model::{AnalyzerModel, ModelError};
use crate::table::{Entry, TruthTable};
use crate::var::{is_valid_name, Var, MAX_VAR_WIDTH};
use logisim_core::{Location, Value};
use logisim_format::{ComponentDef, Error as FormatError, ProjectFile};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum CircuitError {
    NoSuchCircuit(String),
    /// A pin's label cannot be used as a variable name.
    BadLabel(String),
    NoInputs,
    NoOutputs,
    TooManyInputs {
        bits: usize,
        max: usize,
    },
    /// The circuit never settles for some input (a loop or an oscillator), so it is not
    /// combinational.
    Unstable {
        row: usize,
    },
    Model(ModelError),
    Format(FormatError),
}

impl fmt::Display for CircuitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CircuitError::NoSuchCircuit(n) => write!(f, "there is no circuit named '{n}'"),
            CircuitError::BadLabel(l) => write!(
                f,
                "the pin label '{l}' cannot be used as a variable name (use letters, digits and _, starting with a letter)"
            ),
            CircuitError::NoInputs => write!(f, "the circuit has no labelled input pins"),
            CircuitError::NoOutputs => write!(f, "the circuit has no labelled output pins"),
            CircuitError::TooManyInputs { bits, max } => write!(
                f,
                "the circuit has {bits} input bits, but a truth table can have at most {max}"
            ),
            CircuitError::Unstable { row } => write!(
                f,
                "the circuit does not settle for input row {row}; only combinational circuits can be analysed"
            ),
            CircuitError::Model(e) => e.fmt(f),
            CircuitError::Format(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for CircuitError {}

impl From<ModelError> for CircuitError {
    fn from(e: ModelError) -> Self {
        CircuitError::Model(e)
    }
}
impl From<FormatError> for CircuitError {
    fn from(e: FormatError) -> Self {
        CircuitError::Format(e)
    }
}

struct PinInfo {
    index: usize,
    var: Var,
    at: (i32, i32),
}

fn labelled_pins(
    project: &ProjectFile,
    circuit: &str,
) -> Result<(Vec<PinInfo>, Vec<PinInfo>), CircuitError> {
    let def = project
        .circuit(circuit)
        .ok_or_else(|| CircuitError::NoSuchCircuit(circuit.to_string()))?;
    let mut ins = vec![];
    let mut outs = vec![];
    for (index, c) in def.components.iter().enumerate() {
        if c.library != "Wiring" || c.part != "Pin" {
            continue;
        }
        let label = match c.options.string("label") {
            Ok(Some(l)) if !l.trim().is_empty() => l.trim().to_string(),
            _ => continue,
        };
        if !is_valid_name(&label) {
            return Err(CircuitError::BadLabel(label));
        }
        let width = c.options.int("width", 1, 64).ok().flatten().unwrap_or(1) as u32;
        if width > MAX_VAR_WIDTH {
            return Err(CircuitError::BadLabel(format!(
                "{label} ({width} bits is too wide)"
            )));
        }
        let info = PinInfo {
            index,
            var: Var::new(&label, width),
            at: c.at,
        };
        if c.options.boolean("output").ok().flatten() == Some(true) {
            outs.push(info);
        } else {
            ins.push(info);
        }
    }
    for list in [&mut ins, &mut outs] {
        list.sort_by_key(|p| (p.at.1, p.at.0));
    }
    Ok((ins, outs))
}

/// Run `circuit` of `project` for every input combination and collect the results.
pub fn analyze_circuit(
    project: &ProjectFile,
    circuit: &str,
) -> Result<AnalyzerModel, CircuitError> {
    let (ins, outs) = labelled_pins(project, circuit)?;
    if ins.is_empty() {
        return Err(CircuitError::NoInputs);
    }
    if outs.is_empty() {
        return Err(CircuitError::NoOutputs);
    }
    let in_bits: usize = ins.iter().map(|p| p.var.width as usize).sum();
    if in_bits > TruthTable::MAX_INPUTS {
        return Err(CircuitError::TooManyInputs {
            bits: in_bits,
            max: TruthTable::MAX_INPUTS,
        });
    }
    let out_bits: usize = outs.iter().map(|p| p.var.width as usize).sum();
    let mut table = TruthTable::new(in_bits, out_bits);
    let mut work = project.clone();
    let def_index = work
        .circuits
        .iter()
        .position(|c| c.name == circuit)
        .expect("circuit exists");
    for row in 0..table.row_count() {
        // set each input pin to its slice of the row number (first variable = top bits)
        let mut shift = in_bits;
        for p in &ins {
            shift -= p.var.width as usize;
            let v = (row >> shift) & ((1usize << p.var.width) - 1);
            let comp: &mut ComponentDef = &mut work.circuits[def_index].components[p.index];
            comp.options.set("value", v as i64);
        }
        let mut c = work.instantiate(circuit)?;
        c.reset();
        if c.is_oscillating() {
            return Err(CircuitError::Unstable { row });
        }
        let mut col = 0;
        for p in &outs {
            let v: Value = c.value_at(Location::new(p.at.0, p.at.1));
            for b in (0..p.var.width).rev() {
                let bit = v.bit(b as u8);
                table.set(
                    row,
                    col,
                    if bit == Value::TRUE {
                        Entry::One
                    } else if bit == Value::FALSE {
                        Entry::Zero
                    } else {
                        Entry::BusError
                    },
                );
                col += 1;
            }
        }
    }
    let in_vars: Vec<Var> = ins.into_iter().map(|p| p.var).collect();
    let out_vars: Vec<Var> = outs.into_iter().map(|p| p.var).collect();
    Ok(AnalyzerModel::from_parts(&in_vars, &out_vars, table)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{build_circuit, BuildOptions};
    use logisim_format::CircuitDef;

    fn project_with(def: CircuitDef) -> ProjectFile {
        let mut p = ProjectFile::new("t", &def.name);
        p.circuits.push(def);
        p
    }

    fn pin(label: &str, y: i32, output: bool, width: i64, x: i32) -> ComponentDef {
        ComponentDef::new("Wiring", "Pin", x, y)
            .unwrap()
            .option("label", label)
            .option("output", output)
            .option("width", width)
    }

    #[test]
    fn analyses_a_generated_circuit_back_to_its_table() {
        let mut m = AnalyzerModel::new();
        for n in ["a", "b", "c"] {
            m.add_input(Var::new(n, 1)).unwrap();
        }
        m.add_output(Var::new("y", 1)).unwrap();
        m.add_output(Var::new("z", 1)).unwrap();
        m.set_expression_text("y", "a b + ~a c").unwrap();
        m.set_expression_text("z", "a ^ b ^ c").unwrap();
        for opts in [
            BuildOptions::default(),
            BuildOptions {
                nand_only: true,
                two_input_only: true,
                ..Default::default()
            },
        ] {
            let p = project_with(build_circuit(&m, &opts).unwrap());
            let back = analyze_circuit(&p, "Analyzed").unwrap();
            assert_eq!(back.input_bits(), ["a", "b", "c"]);
            assert_eq!(back.output_bits(), ["y", "z"]);
            assert_eq!(back.table(), m.table());
        }
    }

    #[test]
    fn wide_pins_become_bus_variables() {
        // a 2-bit pin through a 2-bit NOT gate
        let not = ComponentDef::new("Gates", "NOT Gate", 200, 100)
            .unwrap()
            .option("width", 2);
        let in_loc = not.port_location(1).unwrap();
        let out_loc = not.port_location(0).unwrap();
        let def = CircuitDef {
            name: "inv".into(),
            interface: vec![],
            components: vec![
                pin("A", 100, false, 2, in_loc.x - 60),
                not,
                pin("Y", 100, true, 2, out_loc.x + 60),
            ],
            wires: vec![
                logisim_format::WireDef {
                    a: (in_loc.x - 60, 100),
                    b: (in_loc.x, in_loc.y),
                },
                logisim_format::WireDef {
                    a: (out_loc.x, out_loc.y),
                    b: (out_loc.x + 60, 100),
                },
            ],
        };
        let m = analyze_circuit(&project_with(def), "inv").unwrap();
        assert_eq!(m.inputs().vars(), [Var::new("A", 2)]);
        assert_eq!(m.output_bits(), ["Y[1]", "Y[0]"]);
        // Y = ~A, bit by bit
        for row in 0..4usize {
            let y1 = m.table().get(row, 0);
            let y0 = m.table().get(row, 1);
            assert_eq!(y1, Entry::from_bool(row >> 1 & 1 == 0), "row {row}");
            assert_eq!(y0, Entry::from_bool(row & 1 == 0), "row {row}");
        }
    }

    #[test]
    fn floating_outputs_are_bus_errors() {
        let def = CircuitDef {
            name: "open".into(),
            interface: vec![],
            components: vec![pin("a", 0, false, 1, 0), pin("y", 100, true, 1, 100)],
            wires: vec![],
        };
        let m = analyze_circuit(&project_with(def), "open").unwrap();
        assert_eq!(m.table().get(0, 0), Entry::BusError);
    }

    #[test]
    fn pins_are_ordered_top_to_bottom_and_unlabelled_pins_are_ignored() {
        let def = CircuitDef {
            name: "c".into(),
            interface: vec![],
            components: vec![
                pin("second", 50, false, 1, 0),
                pin("first", 10, false, 1, 0),
                ComponentDef::new("Wiring", "Pin", 0, 90).unwrap(),
                pin("y", 10, true, 1, 100),
            ],
            wires: vec![],
        };
        let m = analyze_circuit(&project_with(def), "c").unwrap();
        assert_eq!(m.input_bits(), ["first", "second"]);
    }

    #[test]
    fn reports_problems_clearly() {
        let p = project_with(CircuitDef {
            name: "c".into(),
            interface: vec![],
            components: vec![pin("1bad", 0, false, 1, 0), pin("y", 0, true, 1, 100)],
            wires: vec![],
        });
        assert!(matches!(
            analyze_circuit(&p, "c"),
            Err(CircuitError::BadLabel(_))
        ));
        assert!(matches!(
            analyze_circuit(&p, "zzz"),
            Err(CircuitError::NoSuchCircuit(_))
        ));
        let p = project_with(CircuitDef {
            name: "c".into(),
            interface: vec![],
            components: vec![pin("y", 0, true, 1, 100)],
            wires: vec![],
        });
        assert_eq!(analyze_circuit(&p, "c"), Err(CircuitError::NoInputs));
        let p = project_with(CircuitDef {
            name: "c".into(),
            interface: vec![],
            components: vec![pin("a", 0, false, 1, 0)],
            wires: vec![],
        });
        assert_eq!(analyze_circuit(&p, "c"), Err(CircuitError::NoOutputs));
    }

    #[test]
    fn a_feedback_loop_terminates() {
        // NOT gate whose output feeds its own input
        let not = ComponentDef::new("Gates", "NOT Gate", 100, 0).unwrap();
        let (i, o) = (not.port_location(1).unwrap(), not.port_location(0).unwrap());
        let def = CircuitDef {
            name: "ring".into(),
            interface: vec![],
            components: vec![pin("a", 200, false, 1, 0), not, pin("y", 200, true, 1, 300)],
            wires: vec![
                logisim_format::WireDef {
                    a: (i.x, i.y),
                    b: (i.x, i.y - 40),
                },
                logisim_format::WireDef {
                    a: (i.x, i.y - 40),
                    b: (o.x + 20, i.y - 40),
                },
                logisim_format::WireDef {
                    a: (o.x + 20, i.y - 40),
                    b: (o.x + 20, o.y),
                },
                logisim_format::WireDef {
                    a: (o.x, o.y),
                    b: (o.x + 20, o.y),
                },
            ],
        };
        // a lone inverter loop settles at an error value; either way analysis must finish
        let r = analyze_circuit(&project_with(def), "ring");
        assert!(r.is_ok() || matches!(r, Err(CircuitError::Unstable { .. })));
    }
}
