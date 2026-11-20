//! Problems worth pointing out on the canvas: unconnected inputs and bit-width mismatches.
//! Pure functions over the circuit definition so they are easy to test; the canvas only draws
//! the badges and explains the fix in a tooltip.

use crate::model::P;
use crate::nets::NetAnalysis;
use logisim_core::{BitWidth, Port, PortKind};
use logisim_format::CircuitDef;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    UnconnectedInput,
    WidthMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub at: P,
    /// Index of the component the port belongs to.
    pub comp: usize,
    pub kind: Kind,
    /// What is wrong, in plain words.
    pub title: String,
    /// What to do about it.
    pub fix: String,
}

/// Geometry of one placed part, as the editor caches it.
pub struct PartPorts<'a> {
    pub name: &'a str,
    pub ports: &'a [Port],
    /// Absolute location of each port.
    pub abs: &'a [P],
}

/// Parts whose ports are meant to be left open or that adapt to what they touch.
fn exempt(name: &str) -> bool {
    matches!(
        name,
        "Do Not Connect" | "Tunnel" | "Pull Resistor" | "Probe" | "Splitter" | "Power On Reset"
    )
}

pub fn diagnose(c: &CircuitDef, parts: &[PartPorts], nets: &NetAnalysis) -> Vec<Problem> {
    let mut out = vec![];
    let in_net: HashSet<P> = nets
        .nets
        .iter()
        .flat_map(|n| n.points.iter().copied())
        .collect();
    // how many ports sit at each point (two ports touching directly are connected)
    let mut port_count: HashMap<P, usize> = HashMap::new();
    for p in parts {
        for a in p.abs {
            *port_count.entry(*a).or_default() += 1;
        }
    }
    // ---- unconnected inputs
    for (ci, part) in parts.iter().enumerate() {
        if exempt(part.name) {
            continue;
        }
        for (i, port) in part.ports.iter().enumerate() {
            if port.kind != PortKind::Input {
                continue;
            }
            let at = part.abs[i];
            let touching = port_count.get(&at).copied().unwrap_or(0) > 1;
            if !in_net.contains(&at) && !touching {
                out.push(Problem {
                    at,
                    comp: ci,
                    kind: Kind::UnconnectedInput,
                    title: format!("An input of {} is not connected", part.name),
                    fix: "Draw a wire to it, or tie it to a Constant, Power or Ground part. An unconnected input floats and can make the output unknown.".into(),
                });
            }
        }
    }
    // ---- width mismatches: ports on one net must agree
    for net in &nets.nets {
        let mut widths: Vec<(usize, P, BitWidth)> = vec![];
        for (ci, part) in parts.iter().enumerate() {
            for (i, port) in part.ports.iter().enumerate() {
                if port.width != Port::ANY_WIDTH && net.points.contains(&part.abs[i]) {
                    widths.push((ci, part.abs[i], port.width));
                }
            }
        }
        let mut count: HashMap<BitWidth, usize> = HashMap::new();
        for (_, _, w) in &widths {
            *count.entry(*w).or_default() += 1;
        }
        if count.len() < 2 {
            continue;
        }
        // the width most ports agree on is "right" (ties: the narrower); flag the others
        let majority = count
            .iter()
            .max_by_key(|(w, n)| (**n, std::cmp::Reverse(w.get())))
            .map(|(w, _)| *w)
            .unwrap_or(BitWidth::ONE);
        let tie = count.values().filter(|n| **n == count[&majority]).count() > 1;
        for (ci, at, w) in widths {
            if w != majority || tie {
                out.push(Problem {
                    at,
                    comp: ci,
                    kind: Kind::WidthMismatch,
                    title: format!("{}-bit port on a {}-bit wire", w.get(), majority.get()),
                    fix: "Make the bit widths match: change the part's \"Data bits\" option, or use a Splitter to join or split a bus.".into(),
                });
            }
        }
    }
    let _ = c;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nets::HueMemory;
    use logisim_format::{ComponentDef, WireDef};

    struct Built {
        c: CircuitDef,
        ports: Vec<Vec<Port>>,
        abs: Vec<Vec<P>>,
        names: Vec<String>,
    }

    fn build(defs: Vec<ComponentDef>, wires: &[(P, P)]) -> Built {
        let mut ports = vec![];
        let mut abs = vec![];
        let mut names = vec![];
        for d in &defs {
            let comp = d.build().unwrap();
            let ps = comp.ports();
            abs.push(
                ps.iter()
                    .map(|p| {
                        let o = d.facing.rotate(p.offset);
                        (d.at.0 + o.x, d.at.1 + o.y)
                    })
                    .collect(),
            );
            names.push(comp.name().to_string());
            ports.push(ps);
        }
        Built {
            c: CircuitDef {
                name: "t".into(),
                interface: vec![],
                components: defs,
                wires: wires
                    .iter()
                    .map(|(a, b)| WireDef { a: *a, b: *b })
                    .collect(),
            },
            ports,
            abs,
            names,
        }
    }

    fn run(b: &Built) -> Vec<Problem> {
        let parts: Vec<PartPorts> = (0..b.names.len())
            .map(|i| PartPorts {
                name: &b.names[i],
                ports: &b.ports[i],
                abs: &b.abs[i],
            })
            .collect();
        let nets = HueMemory::default().analyze(&b.c, &b.abs.clone());
        diagnose(&b.c, &parts, &nets)
    }

    fn comp(lib: &str, part: &str, x: i32, y: i32) -> ComponentDef {
        ComponentDef::new(lib, part, x, y).unwrap()
    }

    #[test]
    fn unconnected_gate_input_is_flagged_and_connected_ones_are_not() {
        // AND at (100,100): inputs (50,80) and (50,120). Wire only the first.
        let b = build(
            vec![comp("Gates", "AND Gate", 100, 100)],
            &[((0, 80), (50, 80))],
        );
        let p = run(&b);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].kind, p[0].at), (Kind::UnconnectedInput, (50, 120)));
        assert!(p[0].fix.contains("Constant"));
        let both = build(
            vec![comp("Gates", "AND Gate", 100, 100)],
            &[((0, 80), (50, 80)), ((0, 120), (50, 120))],
        );
        assert!(run(&both).is_empty());
    }

    #[test]
    fn outputs_and_exempt_parts_are_never_flagged() {
        let b = build(
            vec![
                comp("Wiring", "Probe", 0, 0),
                comp("Wiring", "Tunnel", 100, 0),
                comp("Wiring", "Pin", 200, 0),
            ],
            &[],
        );
        assert!(run(&b).is_empty(), "{:?}", run(&b));
    }

    #[test]
    fn ports_touching_directly_count_as_connected() {
        // output Pin (input-kind port) placed exactly on a Pin's output port
        let b = build(
            vec![
                comp("Wiring", "Pin", 0, 0),
                comp("Wiring", "Pin", 0, 0).option("output", true),
            ],
            &[],
        );
        assert!(run(&b).is_empty());
    }

    #[test]
    fn width_mismatch_is_reported_on_the_minority_ports() {
        // two 1-bit pins and one 4-bit output pin all on one wire
        let wires = [((0, 0), (100, 0))];
        let b = build(
            vec![
                comp("Wiring", "Pin", 0, 0),
                comp("Wiring", "Pin", 100, 0)
                    .option("output", true)
                    .option("width", 4),
                comp("Gates", "AND Gate", 100, 0),
            ],
            &wires,
        );
        let p: Vec<_> = run(&b)
            .into_iter()
            .filter(|p| p.kind == Kind::WidthMismatch)
            .collect();
        assert!(!p.is_empty());
        assert!(p.iter().any(|p| p.title.starts_with("4-bit")), "{p:?}");
        assert!(p.iter().all(|p| p.fix.contains("Splitter")));
    }

    #[test]
    fn matching_widths_are_fine() {
        let b = build(
            vec![
                comp("Wiring", "Pin", 0, 0).option("width", 4),
                comp("Wiring", "Pin", 100, 0)
                    .option("output", true)
                    .option("width", 4),
            ],
            &[((0, 0), (100, 0))],
        );
        assert!(run(&b).is_empty());
    }
}
