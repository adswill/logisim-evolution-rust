//! The test-vector runner: load a vector file, run it against a fresh copy of the circuit and
//! show a pass/fail line per row.

use eframe::egui::{self, Color32, RichText};
use logisim_core::Value;
use logisim_format::ProjectFile;
use logisim_log::{CircuitRig, Radix, TestError, TestRun, TestVector};

/// Result marks. Colour backs them up but never replaces them.
const PASS: &str = "✔ Pass";
const FAIL: &str = "✘ Fail";
const PENDING: &str = "– Not run";

#[derive(Default)]
pub struct VectorPanel {
    pub path: String,
    run: Option<TestRun>,
    pub error: Option<String>,
    only_failures: bool,
    /// Circuit the current results belong to.
    for_circuit: String,
}

fn radix_of(base: u32) -> Radix {
    match base {
        2 => Radix::Binary,
        8 => Radix::Octal,
        16 => Radix::Hex,
        _ => Radix::DecimalUnsigned,
    }
}

impl VectorPanel {
    /// Load a vector from file text; keeps the previous vector if it does not parse.
    pub fn load_text(&mut self, text: &str) {
        match TestVector::parse(text) {
            Ok(v) => {
                self.run = Some(TestRun::new(v));
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn load_file(&mut self, path: &str) {
        match std::fs::read_to_string(path) {
            Ok(t) => {
                self.load_text(&t);
                self.path = path.to_string();
            }
            Err(e) => self.error = Some(format!("Cannot read {path}: {e}")),
        }
    }

    /// Run every row against a fresh copy of `circuit` (not the live simulation).
    pub fn run(&mut self, project: &ProjectFile, circuit: &str) {
        let Some(run) = self.run.as_mut() else { return };
        let rig = CircuitRig::new(project, circuit, run.vector());
        match rig {
            Ok(mut rig) => {
                run.run(&mut rig);
                self.error = None;
                self.for_circuit = circuit.to_string();
            }
            Err(TestError(e)) => self.error = Some(e),
        }
    }

    #[cfg(test)]
    pub fn summary(&self) -> Option<(usize, usize, usize)> {
        self.run
            .as_ref()
            .map(|r| (r.pass(), r.fail(), r.vector().row_count()))
    }

    pub fn show(&mut self, ui: &mut egui::Ui, project: &ProjectFile, circuit: &str) {
        ui.horizontal(|ui| {
            ui.label("Vector file:");
            ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .desired_width(260.0)
                    .hint_text("path to a .txt test vector"),
            );
            if ui.button("Open…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Test vector", &["txt", "vec"])
                    .pick_file()
                {
                    self.load_file(&p.to_string_lossy());
                }
            }
            if ui
                .button("Load")
                .on_hover_text("Reload the file named in the box")
                .clicked()
            {
                let p = self.path.clone();
                self.load_file(&p);
            }
        });
        let loaded = self.run.is_some();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(loaded, egui::Button::new("Run all rows"))
                .clicked()
            {
                self.run(project, circuit);
            }
            ui.checkbox(&mut self.only_failures, "Only failures");
            ui.weak(format!(
                "Runs on a fresh copy of “{circuit}”, not on the live simulation."
            ));
        });
        if let Some(e) = &self.error {
            ui.colored_label(error_colour(ui), format!("⚠ {e}"));
        }
        let Some(run) = self.run.as_ref() else {
            ui.weak("Open a test vector file to begin. A vector is a table: column names (the labels of your pins) on the first line, then one row of values per test.");
            return;
        };
        let (pass, fail, total) = (run.pass(), run.fail(), run.vector().row_count());
        let (text, col) = if run.done() == 0 {
            (
                format!("{total} rows, not run yet"),
                ui.visuals().weak_text_color(),
            )
        } else if fail == 0 && pass == total {
            (format!("✔ All {total} rows passed"), ok_colour(ui))
        } else {
            (
                format!("✘ {fail} failed, {pass} passed, of {total} rows"),
                error_colour(ui),
            )
        };
        ui.label(RichText::new(text).strong().color(col));
        ui.separator();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("vector_rows")
                    .striped(true)
                    .num_columns(3 + run.vector().column_name.len())
                    .show(ui, |ui| {
                        for h in ["Result", "Row", "Line"] {
                            ui.strong(h);
                        }
                        for n in &run.vector().column_name {
                            ui.strong(n);
                        }
                        ui.end_row();
                        let v = run.vector();
                        for row in 0..v.row_count() {
                            let report = run.result(row);
                            let failed = report.is_some();
                            let ran = row < run.done() || failed;
                            if self.only_failures && !failed {
                                continue;
                            }
                            let (mark, mark_col) = match (ran, failed) {
                                (false, _) => (PENDING, ui.visuals().weak_text_color()),
                                (true, false) => (PASS, ok_colour(ui)),
                                (true, true) => (FAIL, error_colour(ui)),
                            };
                            ui.label(RichText::new(mark).color(mark_col));
                            ui.label((row + 1).to_string());
                            ui.label(v.line_numbers[row].to_string());
                            for col in 0..v.column_name.len() {
                                let expected = cell_text(v, row, col);
                                match report.and_then(|r| r.iter().find(|l| l.column == col)) {
                                    Some(l) => ui.label(
                                        RichText::new(format!(
                                            "{expected} (got {})",
                                            radix_of(v.column_radix[col]).format(&l.computed)
                                        ))
                                        .color(error_colour(ui))
                                        .strong(),
                                    ),
                                    None => ui.label(expected),
                                };
                            }
                            ui.end_row();
                        }
                    });
            });
    }
}

fn cell_text(v: &TestVector, row: usize, col: usize) -> String {
    if v.is_dont_care(row, col) {
        "<DC>".into()
    } else if v.is_floating(row, col) {
        "<FLOAT>".into()
    } else {
        let val: Value = v.data[row][col];
        radix_of(v.column_radix[col]).format(&val)
    }
}

fn ok_colour(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(120, 220, 150)
    } else {
        Color32::from_rgb(0, 110, 50)
    }
}

fn error_colour(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(255, 120, 120)
    } else {
        Color32::from_rgb(190, 20, 20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_format::{CircuitDef, ComponentDef};

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

    #[test]
    fn run_reports_per_row_results() {
        let p = and_project();
        let mut panel = VectorPanel::default();
        panel.load_text("a b q\n0 0 0\n1 1 1\n1 1 0\n");
        assert_eq!(panel.summary(), Some((0, 0, 3)));
        panel.run(&p, "main");
        assert_eq!(panel.summary(), Some((2, 1, 3)));
        let run = panel.run.as_ref().unwrap();
        assert!(run.result(2).is_some() && run.result(0).is_none());
        assert_eq!(cell_text(run.vector(), 2, 2), "0");
    }

    #[test]
    fn problems_become_messages_and_keep_the_old_vector() {
        let p = and_project();
        let mut panel = VectorPanel::default();
        panel.load_text("a b q\n0 0 0\n");
        panel.load_text("a b q\n0 0\n");
        assert!(panel
            .error
            .as_deref()
            .unwrap()
            .contains("not enough values"));
        assert_eq!(panel.summary(), Some((0, 0, 1)));
        panel.load_text("a x q\n0 0 0\n");
        panel.run(&p, "main");
        assert_eq!(panel.error.as_deref(), Some("Column x has no matching pin"));
        panel.load_file("/nonexistent/vector.txt");
        assert!(panel.error.as_deref().unwrap().starts_with("Cannot read"));
    }

    #[test]
    fn dont_care_and_float_cells_keep_their_names() {
        let v = TestVector::parse("a b q\n1 1 <DC>\n0 0 <FLOAT>\n").unwrap();
        assert_eq!(cell_text(&v, 0, 2), "<DC>");
        assert_eq!(cell_text(&v, 1, 2), "<FLOAT>");
    }
}
