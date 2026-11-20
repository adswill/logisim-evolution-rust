//! Problems worth pointing out on the canvas: unconnected required inputs, bit-width
//! mismatches, and (while simulating) inputs left floating whose part now outputs X or E.
//! Issues are grouped per part so the canvas can draw ONE small badge per part and list every
//! issue, with its fix, in the tooltip. Pure functions, so they are easy to test.

use crate::model::P;
use crate::nets::NetAnalysis;
use logisim_core::{BitWidth, Port, PortKind};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    UnconnectedInput,
    WidthMismatch,
}

/// One thing wrong with a port of a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub at: P,
    pub kind: Kind,
    /// What is wrong, in plain words.
    pub title: String,
    /// What to do about it.
    pub fix: String,
}

/// All issues of one part (never empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Index of the component in the circuit.
    pub comp: usize,
    pub issues: Vec<Issue>,
}

impl Problem {
    /// Tooltip text: every issue, then each distinct fix once.
    pub fn tooltip(&self, part_name: &str) -> Vec<String> {
        let n = self.issues.len();
        let mut lines = vec![format!(
            "{part_name}: {n} problem{}",
            if n == 1 { "" } else { "s" }
        )];
        lines.extend(self.issues.iter().map(|i| format!("• {}", i.title)));
        let mut fixes: Vec<&str> = vec![];
        for i in &self.issues {
            if !fixes.contains(&i.fix.as_str()) {
                fixes.push(&i.fix);
            }
        }
        lines.push(String::new());
        lines.extend(fixes.into_iter().map(|f| format!("Fix: {f}")));
        lines
    }
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

/// `output_unknown(i)` says whether part `i` currently drives X or E on an output (live
/// simulation); pass `|_| false` when not simulating.
pub fn diagnose(
    parts: &[PartPorts],
    nets: &NetAnalysis,
    output_unknown: &dyn Fn(usize) -> bool,
) -> Vec<Problem> {
    let in_net: HashSet<P> = nets
        .nets
        .iter()
        .flat_map(|n| n.points.iter().copied())
        .collect();
    // two ports touching directly are connected
    let mut port_count: HashMap<P, usize> = HashMap::new();
    for p in parts {
        for a in p.abs {
            *port_count.entry(*a).or_default() += 1;
        }
    }
    let mut by_part: Vec<Vec<Issue>> = vec![vec![]; parts.len()];

    // ---- unconnected inputs
    for (ci, part) in parts.iter().enumerate() {
        if exempt(part.name) {
            continue;
        }
        let inputs = part
            .ports
            .iter()
            .filter(|p| p.kind == PortKind::Input)
            .count();
        let mut nth = 0;
        for (i, port) in part.ports.iter().enumerate() {
            if port.kind != PortKind::Input {
                continue;
            }
            nth += 1;
            let at = part.abs[i];
            let connected = in_net.contains(&at) || port_count.get(&at).copied().unwrap_or(0) > 1;
            if connected {
                continue;
            }
            // Required inputs always warn. Optional ones (enable, clear, load...) float by default;
            // they only matter once the part is visibly producing X or E.
            let bad_output = output_unknown(ci);
            if port.required || bad_output {
                let which = if inputs > 1 {
                    format!("Input {nth} of {}", part.name)
                } else {
                    format!("The input of {}", part.name)
                };
                let (title, fix) = if port.required {
                    (
                        format!("{which} is not connected"),
                        "Draw a wire to it, or tie it to a Constant, Power or Ground part."
                            .to_string(),
                    )
                } else {
                    (
                        format!("{which} is not connected and the output is unknown"),
                        "Connect it, or tie it to a Constant, Power or Ground part, so the output gets a defined value.".to_string(),
                    )
                };
                by_part[ci].push(Issue {
                    at,
                    kind: Kind::UnconnectedInput,
                    title,
                    fix,
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
                by_part[ci].push(Issue {
                    at,
                    kind: Kind::WidthMismatch,
                    title: format!("A {}-bit port is on a {}-bit wire", w.get(), majority.get()),
                    fix: "Make the bit widths match: change the part's \"Data bits\" option, or use a Splitter to join or split a bus.".into(),
                });
            }
        }
    }

    by_part
        .into_iter()
        .enumerate()
        .filter(|(_, v)| !v.is_empty())
        .map(|(comp, issues)| Problem { comp, issues })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nets::HueMemory;
    use crate::sim::Sim;
    use logisim_format::{CircuitDef, ComponentDef, ProjectFile, WireDef};

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
                        let o = d.facing.place(d.mirror, p.offset);
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

    fn run_with(b: &Built, unknown: &dyn Fn(usize) -> bool) -> Vec<Problem> {
        let parts: Vec<PartPorts> = (0..b.names.len())
            .map(|i| PartPorts {
                name: &b.names[i],
                ports: &b.ports[i],
                abs: &b.abs[i],
            })
            .collect();
        let nets = HueMemory::default().analyze(&b.c, &b.abs.clone());
        diagnose(&parts, &nets, unknown)
    }

    fn run(b: &Built) -> Vec<Problem> {
        run_with(b, &|_| false)
    }

    fn comp(lib: &str, part: &str, x: i32, y: i32) -> ComponentDef {
        ComponentDef::new(lib, part, x, y).unwrap()
    }

    #[test]
    fn lone_two_input_and_gives_one_badge_listing_both_inputs() {
        let b = build(vec![comp("Gates", "AND Gate", 100, 100)], &[]);
        let p = run(&b);
        assert_eq!(p.len(), 1, "one badge per part");
        assert_eq!(p[0].issues.len(), 2);
        let t = p[0].tooltip("AND Gate").join("\n");
        assert!(
            t.contains("Input 1 of AND Gate") && t.contains("Input 2 of AND Gate"),
            "{t}"
        );
        assert_eq!(
            t.matches("Fix:").count(),
            1,
            "the same fix is listed once: {t}"
        );
    }

    #[test]
    fn connecting_one_input_leaves_one_issue_and_both_leaves_none() {
        let one = build(
            vec![comp("Gates", "AND Gate", 100, 100)],
            &[((0, 80), (50, 80))],
        );
        let p = run(&one);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].issues.len(), p[0].issues[0].at), (1, (50, 120)));
        let both = build(
            vec![comp("Gates", "AND Gate", 100, 100)],
            &[((0, 80), (50, 80)), ((0, 120), (50, 120))],
        );
        assert!(run(&both).is_empty());
    }

    #[test]
    fn a_fresh_counter_alone_has_no_badges_even_while_simulating() {
        let b = build(vec![comp("Memory", "Counter", 100, 100)], &[]);
        assert!(run(&b).is_empty(), "optional control pins must not warn");
        // ... and with the live simulation of the unconnected counter
        let mut project = ProjectFile::new("t", "t");
        project.circuits.push(b.c.clone());
        let mut sim = Sim::build(&project, "t").unwrap();
        let mut pts = vec![];
        for a in &b.abs[0] {
            pts.push(logisim_core::Location::new(a.0, a.1));
        }
        sim.snapshot(pts.clone());
        let ports = &b.ports[0];
        let unknown = |_: usize| {
            ports.iter().enumerate().any(|(i, p)| {
                p.kind == PortKind::Output
                    && sim
                        .value_at(pts[i])
                        .is_some_and(|v| v.is_error() || v.is_unknown())
            })
        };
        assert!(
            run_with(&b, &unknown).is_empty(),
            "a counter alone must stay quiet while simulating"
        );
    }

    #[test]
    fn optional_input_warns_only_when_the_output_is_unknown() {
        let b = build(vec![comp("Memory", "Counter", 100, 100)], &[]);
        let p = run_with(&b, &|_| true);
        assert_eq!(p.len(), 1);
        assert!(p[0]
            .issues
            .iter()
            .all(|i| i.title.contains("output is unknown")));
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
    fn width_mismatch_is_reported_per_part_and_always_warns() {
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
        let p = run(&b);
        let mism: Vec<_> = p
            .iter()
            .flat_map(|p| p.issues.iter())
            .filter(|i| i.kind == Kind::WidthMismatch)
            .collect();
        assert!(!mism.is_empty());
        assert!(
            mism.iter().any(|i| i.title.starts_with("A 4-bit")),
            "{mism:?}"
        );
        assert!(mism.iter().all(|i| i.fix.contains("Splitter")));
        let mut seen = HashSet::new();
        assert!(
            p.iter().all(|p| seen.insert(p.comp)),
            "no part appears twice"
        );
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
