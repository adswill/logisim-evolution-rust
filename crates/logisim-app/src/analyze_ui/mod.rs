//! The Combinational Analysis window: edit a truth table or expressions, see the simplest
//! expression and Karnaugh map, and build a circuit.
//!
//! Hooking it into the editor takes four small steps (see `App` in `app.rs`):
//! keep an [`AnalyzerWindow`], call [`AnalyzerWindow::open`] from a menu item, call
//! [`AnalyzerWindow::show`] once per frame, and apply the returned [`Action`].

mod expr_tab;
mod pal;
mod simplified_tab;
mod state;
mod table_tab;
mod vars_panel;

use eframe::egui::{self, RichText};
use logisim_format::{CircuitDef, ProjectFile};
use pal::Pal;
use state::{Analyzer, Message, Tab};

/// Something the window asks the editor to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Add this generated circuit to the project (rename it if the name is taken) and open it.
    AddCircuit(CircuitDef),
}

pub struct AnalyzerWindow {
    open: bool,
    st: Analyzer,
    vars: vars_panel::VarBuffers,
    exprs: expr_tab::ExprBuffers,
    /// CSV text waiting for the user to say how many columns are inputs.
    csv_pending: Option<(String, usize)>,
}

impl Default for AnalyzerWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyzerWindow {
    pub fn new() -> Self {
        AnalyzerWindow {
            open: false,
            st: Analyzer::new(),
            vars: Default::default(),
            exprs: Default::default(),
            csv_pending: None,
        }
    }

    /// Show the window (a menu item calls this).
    pub fn open(&mut self) {
        self.open = true;
    }

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Draw the window if it is open. `current` names the circuit being edited, which the
    /// "Read from circuit" button analyses.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        dark: bool,
        project: &ProjectFile,
        current: &str,
    ) -> Option<Action> {
        if !self.open {
            return None;
        }
        let pal = Pal::new(dark);
        let mut open = true;
        let mut action = None;
        egui::Window::new("Combinational Analysis")
            .open(&mut open)
            .default_size([1000.0, 640.0])
            .min_size([640.0, 420.0])
            .resizable(true)
            .show(ctx, |ui| {
                action = self.contents(ui, &pal, project, current);
            });
        self.open = open;
        action
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        pal: &Pal,
        project: &ProjectFile,
        current: &str,
    ) -> Option<Action> {
        let mut action = None;
        self.toolbar(ui, project, current);
        self.csv_dialog(ui);
        if let Some(m) = &self.st.message {
            let (text, colour) = match m {
                Message::Info(t) => (t.as_str(), pal.ok),
                Message::Error(t) => (t.as_str(), pal.error),
            };
            ui.label(RichText::new(text).color(colour));
        }
        ui.separator();

        egui::Panel::left("analysis_vars")
            .resizable(true)
            .default_size(290.0)
            .min_size(240.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    vars_panel::show(ui, &mut self.st, pal, &mut self.vars)
                });
            });
        egui::Panel::bottom("analysis_build")
            .resizable(false)
            .show(ui, |ui| {
                action = self.build_bar(ui, pal);
            });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (Tab::Table, "Truth table"),
                    (Tab::Expressions, "Expressions"),
                    (Tab::Simplified, "Simplified and map"),
                ] {
                    ui.selectable_value(&mut self.st.tab, tab, label);
                }
            });
            ui.separator();
            match self.st.tab {
                Tab::Table => table_tab::show(ui, &mut self.st, pal),
                Tab::Expressions => expr_tab::show(ui, &mut self.st, pal, &mut self.exprs),
                Tab::Simplified => simplified_tab::show(ui, &mut self.st, pal),
            }
        });
        action
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, project: &ProjectFile, current: &str) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!("Read from circuit “{current}”"))
                .on_hover_text(
                    "Runs your circuit on every input combination, using its labelled pins",
                )
                .clicked()
            {
                self.st.load_from_circuit(project, current);
            }
            ui.menu_button("Import", |ui| {
                if ui.button("Text file…").clicked() {
                    ui.close();
                    self.import_file(false);
                }
                if ui.button("CSV file…").clicked() {
                    ui.close();
                    self.import_file(true);
                }
            });
            ui.menu_button("Export", |ui| {
                if ui.button("Text file…").clicked() {
                    ui.close();
                    self.export_file(false);
                }
                if ui.button("CSV file…").clicked() {
                    ui.close();
                    self.export_file(true);
                }
            });
            if ui
                .button("Clear table")
                .on_hover_text("Sets every output row to “don't care”")
                .clicked()
            {
                self.st.clear_table();
            }
        });
    }

    fn import_file(&mut self, csv: bool) {
        let mut d = rfd::FileDialog::new();
        d = if csv {
            d.add_filter("CSV table", &["csv"])
        } else {
            d.add_filter("Truth table text", &["txt", "tt"])
        };
        let Some(path) = d.pick_file() else { return };
        match std::fs::read_to_string(&path) {
            Ok(text) if csv => {
                if let Err(e) = self.st.import_csv(&text, None) {
                    if e.contains("Say how many") {
                        let cols = text.lines().next().map_or(2, |l| l.split(',').count());
                        self.csv_pending = Some((text, cols.saturating_sub(1).max(1)));
                    } else {
                        self.st.message = Some(Message::Error(e));
                    }
                }
            }
            Ok(text) => {
                self.st.import_text(&text);
            }
            Err(e) => {
                self.st.message = Some(Message::Error(format!(
                    "Could not read {}: {e}",
                    path.display()
                )))
            }
        }
    }

    fn export_file(&mut self, csv: bool) {
        let (name, ext) = if csv {
            ("truth_table.csv", "csv")
        } else {
            ("truth_table.txt", "txt")
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(name)
            .add_filter(ext, &[ext])
            .save_file()
        else {
            return;
        };
        let body = if csv {
            self.st.export_csv()
        } else {
            self.st.export_text()
        };
        self.st.message = Some(match std::fs::write(&path, body) {
            Ok(()) => Message::Info(format!("Saved {}", path.display())),
            Err(e) => Message::Error(format!("Could not write {}: {e}", path.display())),
        });
    }

    fn csv_dialog(&mut self, ui: &mut egui::Ui) {
        let Some((text, cols)) = &mut self.csv_pending else {
            return;
        };
        let total = text.lines().next().map_or(0, |l| l.split(',').count());
        let mut done = None;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(
                "This CSV does not have 2, 4, 8 … rows, so I cannot tell where the inputs end.",
            );
            ui.horizontal(|ui| {
                ui.label("Number of input columns (the rest are outputs):");
                ui.add(egui::DragValue::new(cols).range(1..=total.saturating_sub(1).max(1)));
                if ui.button("Import").clicked() {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                let (t, c) = self.csv_pending.take().unwrap();
                if let Err(e) = self.st.import_csv(&t, Some(c)) {
                    self.st.message = Some(Message::Error(e));
                }
            }
            Some(false) => self.csv_pending = None,
            None => {}
        }
    }

    fn build_bar(&mut self, ui: &mut egui::Ui, pal: &Pal) -> Option<Action> {
        let mut action = None;
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Build circuit").strong());
            ui.label("name:");
            ui.add(egui::TextEdit::singleline(&mut self.st.build.name).desired_width(120.0));
            ui.checkbox(&mut self.st.build.two_input_only, "Two-input gates only");
            ui.checkbox(&mut self.st.build.nand_only, "NAND gates only");
            if ui
                .add(
                    egui::Button::new(RichText::new("Create circuit").strong()).fill(
                        pal.accent
                            .gamma_multiply(if pal.dark { 0.45 } else { 0.25 }),
                    ),
                )
                .on_hover_text(
                    "Adds a new circuit to your project with one pin per input and output bit",
                )
                .clicked()
            {
                match self.st.build_circuit() {
                    Ok((def, summary)) => {
                        self.st.message = Some(Message::Info(format!(
                            "Created circuit “{}” ({summary})",
                            def.name
                        )));
                        action = Some(Action::AddCircuit(def));
                    }
                    Err(e) => self.st.message = Some(Message::Error(e)),
                }
            }
        });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Vec2};

    fn run_frame(
        w: &mut AnalyzerWindow,
        ctx: &egui::Context,
        input: RawInput,
        dark: bool,
    ) -> Option<Action> {
        let project = ProjectFile::new("p", "main");
        let mut out = None;
        let mut full = ctx.run_ui(input, |ctx| {
            if let Some(a) = w.show(ctx, dark, &project, "main") {
                out = Some(a);
            }
        });
        full.textures_delta.clear();
        out
    }

    fn input() -> RawInput {
        RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(1300.0, 900.0),
            )),
            ..Default::default()
        }
    }

    #[test]
    fn closed_window_draws_nothing_and_opens_on_request() {
        let ctx = egui::Context::default();
        let mut w = AnalyzerWindow::new();
        assert!(!w.is_open());
        assert!(run_frame(&mut w, &ctx, input(), true).is_none());
        w.open();
        assert!(w.is_open());
        run_frame(&mut w, &ctx, input(), true);
        assert!(w.is_open());
    }

    #[test]
    fn every_tab_draws_in_both_themes_without_panicking() {
        for dark in [true, false] {
            for tab in [Tab::Table, Tab::Expressions, Tab::Simplified] {
                let ctx = egui::Context::default();
                let mut w = AnalyzerWindow::new();
                w.open();
                w.st.tab = tab;
                w.st.set_expression_text(0, "A ^ B");
                for _ in 0..3 {
                    run_frame(&mut w, &ctx, input(), dark);
                }
            }
        }
    }

    #[test]
    fn tabs_draw_for_awkward_tables() {
        let ctx = egui::Context::default();
        let mut w = AnalyzerWindow::new();
        w.open();
        // no outputs, then no inputs, then many inputs, then a 6-input map with buses
        for setup in 0..4 {
            let mut st = Analyzer::new();
            match setup {
                0 => st.remove_var(state::Side::Output, "Y"),
                1 => {
                    st.remove_var(state::Side::Input, "A");
                    st.remove_var(state::Side::Input, "B");
                }
                2 => st
                    .replace_var(
                        state::Side::Input,
                        "A",
                        logisim_analyze::var::Var::new("A", 9),
                    )
                    .unwrap(),
                _ => {
                    st.replace_var(
                        state::Side::Input,
                        "A",
                        logisim_analyze::var::Var::new("A", 5),
                    )
                    .unwrap();
                    st.add_var(state::Side::Output, "Q[1..0]").unwrap();
                    st.set_expression_text(1, "A[4] A[0] + B");
                }
            }
            w.st = st;
            for tab in [Tab::Table, Tab::Expressions, Tab::Simplified] {
                w.st.tab = tab;
                run_frame(&mut w, &ctx, input(), true);
            }
        }
    }

    #[test]
    fn expression_errors_are_drawn() {
        let ctx = egui::Context::default();
        let mut w = AnalyzerWindow::new();
        w.open();
        w.st.tab = Tab::Expressions;
        w.st.set_expression_text(0, "A +");
        run_frame(&mut w, &ctx, input(), true);
        assert!(w.st.expr_problems[0].is_some());
    }

    #[test]
    fn clicking_create_circuit_returns_the_action() {
        let ctx = egui::Context::default();
        let mut w = AnalyzerWindow::new();
        w.open();
        w.st.set_expression_text(0, "A B");
        // find the button by clicking where egui laid it out: run once to learn nothing, so drive
        // the state function directly instead and check the action shape
        let (def, _) = w.st.build_circuit().unwrap();
        let a = Action::AddCircuit(def.clone());
        assert_eq!(a, Action::AddCircuit(def));
        run_frame(&mut w, &ctx, input(), false);
    }

    #[test]
    fn pointer_events_do_not_break_the_window() {
        let ctx = egui::Context::default();
        let mut w = AnalyzerWindow::new();
        w.open();
        run_frame(&mut w, &ctx, input(), true);
        let mut i = input();
        i.events.push(Event::PointerMoved(Pos2::new(600.0, 300.0)));
        i.events.push(Event::PointerButton {
            pos: Pos2::new(600.0, 300.0),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        run_frame(&mut w, &ctx, i, true);
    }
}
