//! Statistics dialog: how many of each component a circuit uses.
//!
//! Three counts per part, like Logisim's dialog:
//! * **Direct**: instances placed in the chosen circuit itself (subcircuit instances included).
//! * **Unique**: primitive parts in the circuit plus each distinct subcircuit it uses, every
//!   subcircuit counted once however often it is placed.
//! * **Total**: primitive parts once every subcircuit instance is fully expanded.
//!
//! Entry point: [`StatsWindow::new`] then [`StatsWindow::show`] every frame while open.

use eframe::egui;
use logisim_format::{ProjectFile, SUBCIRCUIT_LIBRARY};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatRow {
    pub library: String,
    pub part: String,
    pub direct: u64,
    pub unique: u64,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    pub circuit: String,
    /// Sorted by library, then part. Subcircuits are listed under library `"Circuit"` with
    /// their direct count only (their insides are counted under the primitive parts).
    pub rows: Vec<StatRow>,
    pub wires: u64,
}

type Key = (String, String);

/// Count components of `circuit` (and everything it instantiates).
pub fn compute(project: &ProjectFile, circuit: &str) -> Result<Stats, String> {
    let top = project
        .circuit(circuit)
        .ok_or_else(|| format!("There is no circuit named '{circuit}'."))?;

    // Total counts per circuit (memoised), detecting recursion.
    fn total(
        p: &ProjectFile,
        name: &str,
        memo: &mut HashMap<String, BTreeMap<Key, u64>>,
        stack: &mut Vec<String>,
    ) -> Result<BTreeMap<Key, u64>, String> {
        if let Some(m) = memo.get(name) {
            return Ok(m.clone());
        }
        if stack.iter().any(|s| s == name) {
            return Err(format!(
                "Circuit '{name}' contains itself ({}).",
                stack.join(" → ")
            ));
        }
        let c = p
            .circuit(name)
            .ok_or_else(|| format!("There is no circuit named '{name}'."))?;
        stack.push(name.to_string());
        let mut out: BTreeMap<Key, u64> = BTreeMap::new();
        for d in &c.components {
            if d.library == SUBCIRCUIT_LIBRARY {
                for (k, n) in total(p, &d.part, memo, stack)? {
                    *out.entry(k).or_default() += n;
                }
            } else {
                *out.entry((d.library.clone(), d.part.clone())).or_default() += 1;
            }
        }
        stack.pop();
        memo.insert(name.to_string(), out.clone());
        Ok(out)
    }

    let mut memo = HashMap::new();
    let totals = total(project, circuit, &mut memo, &mut vec![])?;

    // Direct counts.
    let mut direct: BTreeMap<Key, u64> = BTreeMap::new();
    for d in &top.components {
        *direct
            .entry((d.library.clone(), d.part.clone()))
            .or_default() += 1;
    }

    // Unique: this circuit's own primitives + each reachable subcircuit's own primitives once.
    let mut reach: BTreeSet<String> = BTreeSet::new();
    let mut todo = vec![circuit.to_string()];
    while let Some(n) = todo.pop() {
        if !reach.insert(n.clone()) {
            continue;
        }
        if let Some(c) = project.circuit(&n) {
            todo.extend(
                c.components
                    .iter()
                    .filter(|d| d.library == SUBCIRCUIT_LIBRARY)
                    .map(|d| d.part.clone()),
            );
        }
    }
    let mut unique: BTreeMap<Key, u64> = BTreeMap::new();
    let mut wires = 0;
    for n in &reach {
        if let Some(c) = project.circuit(n) {
            wires += c.wires.len() as u64;
            for d in c
                .components
                .iter()
                .filter(|d| d.library != SUBCIRCUIT_LIBRARY)
            {
                *unique
                    .entry((d.library.clone(), d.part.clone()))
                    .or_default() += 1;
            }
        }
    }

    let keys: BTreeSet<Key> = direct
        .keys()
        .chain(totals.keys())
        .chain(unique.keys())
        .cloned()
        .collect();
    let rows = keys
        .into_iter()
        .map(|k| StatRow {
            direct: direct.get(&k).copied().unwrap_or(0),
            unique: unique.get(&k).copied().unwrap_or(0),
            total: totals.get(&k).copied().unwrap_or(0),
            library: k.0,
            part: k.1,
        })
        .collect();
    Ok(Stats {
        circuit: circuit.to_string(),
        rows,
        wires,
    })
}

impl Stats {
    /// (direct, unique, total) summed over all rows. Subcircuit rows only count as direct.
    pub fn sums(&self) -> (u64, u64, u64) {
        self.rows.iter().fold((0, 0, 0), |a, r| {
            (a.0 + r.direct, a.1 + r.unique, a.2 + r.total)
        })
    }

    pub fn to_csv(&self) -> String {
        let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut s = String::from("Library,Component,Direct,Unique,Total\n");
        for r in &self.rows {
            s += &format!(
                "{},{},{},{},{}\n",
                q(&r.library),
                q(&r.part),
                r.direct,
                r.unique,
                r.total
            );
        }
        let (d, u, t) = self.sums();
        s += &format!("\"\",\"Total\",{d},{u},{t}\n");
        s
    }
}

/// The dialog. Holds a snapshot; build a new one (or call [`StatsWindow::refresh`]) after edits.
pub struct StatsWindow {
    circuit: String,
    result: Result<Stats, String>,
    filter: String,
}

impl StatsWindow {
    pub fn new(project: &ProjectFile, circuit: &str) -> Self {
        StatsWindow {
            circuit: circuit.to_string(),
            result: compute(project, circuit),
            filter: String::new(),
        }
    }

    /// Recompute from the current document (keeps the filter text).
    pub fn refresh(&mut self, project: &ProjectFile) {
        self.result = compute(project, &self.circuit);
    }

    /// Show the window; clears `open` when the user closes it.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) {
        egui::Window::new(format!("Statistics: {}", self.circuit))
            .open(open)
            .default_width(460.0)
            .show(ctx, |ui| self.ui(ui));
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let stats = match &self.result {
            Ok(s) => s,
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                return;
            }
        };
        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.text_edit_singleline(&mut self.filter);
            if ui.button("Copy as CSV").clicked() {
                ui.ctx().copy_text(stats.to_csv());
            }
        });
        ui.separator();
        let f = self.filter.to_lowercase();
        egui::Grid::new("stats_grid")
            .striped(true)
            .num_columns(5)
            .show(ui, |ui| {
                for h in ["Library", "Component", "Direct", "Unique", "Total"] {
                    ui.strong(h);
                }
                ui.end_row();
                for r in stats.rows.iter().filter(|r| {
                    f.is_empty()
                        || r.part.to_lowercase().contains(&f)
                        || r.library.to_lowercase().contains(&f)
                }) {
                    ui.label(&r.library);
                    ui.label(&r.part);
                    ui.label(r.direct.to_string());
                    ui.label(r.unique.to_string());
                    ui.label(r.total.to_string());
                    ui.end_row();
                }
                let (d, u, t) = stats.sums();
                ui.strong("");
                ui.strong("Total");
                ui.strong(d.to_string());
                ui.strong(u.to_string());
                ui.strong(t.to_string());
                ui.end_row();
            });
        ui.add_space(4.0);
        ui.weak(format!(
            "{} wire segments in the circuit and its subcircuits.",
            stats.wires
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_format::{CircuitDef, ComponentDef};

    fn comp(lib: &str, part: &str) -> ComponentDef {
        ComponentDef {
            library: lib.into(),
            part: part.into(),
            at: (0, 0),
            facing: Default::default(),
            options: Default::default(),
        }
    }

    fn circ(name: &str, comps: Vec<ComponentDef>) -> CircuitDef {
        CircuitDef {
            name: name.into(),
            interface: vec![],
            components: comps,
            wires: vec![],
        }
    }

    fn project() -> ProjectFile {
        let mut p = ProjectFile::new("t", "main");
        let sub = SUBCIRCUIT_LIBRARY;
        p.circuits.push(circ(
            "main",
            vec![
                comp("Gates", "AND Gate"),
                comp(sub, "half"),
                comp(sub, "half"),
            ],
        ));
        p.circuits.push(circ(
            "half",
            vec![comp("Gates", "AND Gate"), comp("Gates", "XOR Gate")],
        ));
        p
    }

    fn row<'a>(s: &'a Stats, part: &str) -> &'a StatRow {
        s.rows.iter().find(|r| r.part == part).unwrap()
    }

    #[test]
    fn direct_unique_and_total_counts() {
        let s = compute(&project(), "main").unwrap();
        let and = row(&s, "AND Gate");
        assert_eq!((and.direct, and.unique, and.total), (1, 2, 3));
        let xor = row(&s, "XOR Gate");
        assert_eq!((xor.direct, xor.unique, xor.total), (0, 1, 2));
        let half = row(&s, "half");
        assert_eq!((half.direct, half.unique, half.total), (2, 0, 0));
    }

    #[test]
    fn recursion_and_missing_circuits_are_reported() {
        let mut p = project();
        p.circuits[1]
            .components
            .push(comp(SUBCIRCUIT_LIBRARY, "main"));
        assert!(compute(&p, "main").unwrap_err().contains("contains itself"));
        assert!(compute(&project(), "nope").is_err());
    }

    #[test]
    fn csv_has_header_and_total() {
        let csv = compute(&project(), "main").unwrap().to_csv();
        assert!(csv.starts_with("Library,Component,Direct,Unique,Total\n"));
        assert!(csv.contains("\"Gates\",\"AND Gate\",1,2,3"));
        assert!(csv.trim_end().ends_with("\"\",\"Total\",3,3,5"));
    }
}
