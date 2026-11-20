//! The Combinational Analysis window: edit a truth table or expressions, see the simplest
//! expression and Karnaugh map, and build a circuit.
//!
//! It is an [`AppWindow`](crate::windows::AppWindow); the editor lists it in the Window menu and
//! the command palette. A circuit it builds is added to the project through the editor.

mod expr_tab;
mod pal;
mod simplified_tab;
mod state;
mod table_tab;
mod vars_panel;

use crate::windows::{AppWindow, WindowEnv, WindowGroup};
use eframe::egui::{self, RichText};
use logisim_format::ProjectFile;
use pal::Pal;
use state::{Analyzer, Message, Tab};

#[derive(Default)]
pub struct AnalysisWindow {
    st: Analyzer,
    vars: vars_panel::VarBuffers,
    exprs: expr_tab::ExprBuffers,
    /// CSV text waiting for the user to say how many columns are inputs.
    csv_pending: Option<(String, usize)>,
}

impl AppWindow for AnalysisWindow {
    fn id(&self) -> &'static str {
        "analysis"
    }
    fn title(&self) -> &str {
        "Combinational Analysis"
    }
    fn group(&self) -> WindowGroup {
        WindowGroup::Tools
    }
    fn description(&self) -> &str {
        "Truth tables, expressions, Karnaugh maps and building circuits from them."
    }
    fn default_size(&self) -> [f32; 2] {
        [1000.0, 640.0]
    }
    fn show(&mut self, ui: &mut egui::Ui, env: &mut WindowEnv) {
        let pal = Pal::new(ui.visuals().dark_mode);
        self.contents(ui, &pal, env);
    }
}

impl AnalysisWindow {
    fn contents(&mut self, ui: &mut egui::Ui, pal: &Pal, env: &mut WindowEnv) {
        self.toolbar(ui, &env.editor.project, &env.editor.current.clone());
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
                self.build_bar(ui, pal, env);
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

    fn build_bar(&mut self, ui: &mut egui::Ui, pal: &Pal, env: &mut WindowEnv) {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Build circuit").strong());
            ui.label("name:");
            ui.add(egui::TextEdit::singleline(&mut self.st.build.name).desired_width(120.0));
            ui.checkbox(&mut self.st.build.two_input_only, "Two-input gates only");
            ui.checkbox(&mut self.st.build.nand_only, "NAND gates only");
            let fill = pal
                .accent
                .gamma_multiply(if pal.dark { 0.45 } else { 0.25 });
            if ui
                .add(
                    egui::Button::new(RichText::new("Create circuit").strong())
                        .fill(fill)
                        .min_size(egui::vec2(0.0, crate::style::MIN_TARGET)),
                )
                .on_hover_text(
                    "Adds a new circuit to your project with one pin per input and output bit",
                )
                .clicked()
            {
                self.create_circuit(env);
            }
        });
    }

    /// Build the circuit from the current table and add it to the project (opening it).
    fn create_circuit(&mut self, env: &mut WindowEnv) {
        match self.st.build_circuit() {
            Ok((def, summary)) => {
                let name = env.editor.add_circuit_def(def);
                env.editor.open_circuit(&name);
                self.st.message = Some(Message::Info(format!(
                    "Created circuit “{name}” ({summary})"
                )));
                *env.status = format!("Added circuit '{name}' from the analysis window.");
            }
            Err(e) => self.st.message = Some(Message::Error(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Editor;
    use crate::windows::Windows;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Vec2};
    use logisim_analyze::var::Var;

    fn frame(
        w: &mut AnalysisWindow,
        ctx: &egui::Context,
        editor: &mut Editor,
        input: RawInput,
        dark: bool,
    ) {
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let mut status = String::new();
        let mut full = ctx.run_ui(input, |ui| {
            let mut env = WindowEnv {
                editor: &mut *editor,
                sim: None,
                status: &mut status,
                ctx,
                frame: 0,
            };
            egui::Window::new("t")
                .default_size([1000.0, 640.0])
                .show(ui, |ui| w.show(ui, &mut env));
        });
        full.textures_delta.clear();
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
    fn is_registered_in_the_tools_group() {
        let w = Windows::default();
        let a = w
            .list
            .iter()
            .find(|w| w.id() == "analysis")
            .expect("registered");
        assert_eq!(a.group(), WindowGroup::Tools);
        assert!(!a.description().is_empty());
    }

    #[test]
    fn every_tab_draws_in_both_themes_without_panicking() {
        for dark in [true, false] {
            for tab in [Tab::Table, Tab::Expressions, Tab::Simplified] {
                let ctx = egui::Context::default();
                let mut ed = Editor::new();
                let mut w = AnalysisWindow::default();
                w.st.tab = tab;
                w.st.set_expression_text(0, "A ^ B");
                for _ in 0..3 {
                    frame(&mut w, &ctx, &mut ed, input(), dark);
                }
            }
        }
    }

    #[test]
    fn tabs_draw_for_awkward_tables() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new();
        let mut w = AnalysisWindow::default();
        // no outputs, no inputs, many inputs, and a 5-input map with a bus output
        for setup in 0..4 {
            let mut st = Analyzer::new();
            match setup {
                0 => st.remove_var(state::Side::Output, "Y"),
                1 => {
                    st.remove_var(state::Side::Input, "A");
                    st.remove_var(state::Side::Input, "B");
                }
                2 => st
                    .replace_var(state::Side::Input, "A", Var::new("A", 9))
                    .unwrap(),
                _ => {
                    st.replace_var(state::Side::Input, "A", Var::new("A", 5))
                        .unwrap();
                    st.add_var(state::Side::Output, "Q[1..0]").unwrap();
                    st.set_expression_text(1, "A[4] A[0] + B");
                }
            }
            w.st = st;
            for tab in [Tab::Table, Tab::Expressions, Tab::Simplified] {
                w.st.tab = tab;
                frame(&mut w, &ctx, &mut ed, input(), true);
            }
        }
    }

    #[test]
    fn expression_errors_are_drawn() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new();
        let mut w = AnalysisWindow::default();
        w.st.tab = Tab::Expressions;
        w.st.set_expression_text(0, "A +");
        frame(&mut w, &ctx, &mut ed, input(), true);
        assert!(w.st.expr_problems[0].is_some());
    }

    #[test]
    fn creating_a_circuit_adds_it_to_the_project_and_opens_it() {
        let mut ed = Editor::new();
        let before = ed.project.circuits.len();
        let mut w = AnalysisWindow::default();
        w.st.set_expression_text(0, "A B");
        let mut status = String::new();
        let mut env = WindowEnv {
            editor: &mut ed,
            sim: None,
            status: &mut status,
            ctx: &egui::Context::default(),
            frame: 0,
        };
        w.create_circuit(&mut env);
        assert_eq!(ed.project.circuits.len(), before + 1);
        assert_eq!(ed.current, "Analyzed");
        assert!(status.contains("Analyzed"));
        // a second build gets its own name instead of overwriting
        let mut env = WindowEnv {
            editor: &mut ed,
            sim: None,
            status: &mut status,
            ctx: &egui::Context::default(),
            frame: 0,
        };
        w.create_circuit(&mut env);
        assert_eq!(ed.project.circuits.len(), before + 2);
        assert_ne!(ed.current, "Analyzed");
        // the generated circuit simulates: it has the labelled pins as its interface
        assert_eq!(ed.circuit().interface.len(), 3);
    }

    #[test]
    fn building_without_outputs_reports_an_error_and_changes_nothing() {
        let mut ed = Editor::new();
        let before = ed.project.circuits.len();
        let mut w = AnalysisWindow::default();
        w.st.remove_var(state::Side::Output, "Y");
        let mut status = String::new();
        let mut env = WindowEnv {
            editor: &mut ed,
            sim: None,
            status: &mut status,
            ctx: &egui::Context::default(),
            frame: 0,
        };
        w.create_circuit(&mut env);
        assert_eq!(ed.project.circuits.len(), before);
        assert!(matches!(w.st.message, Some(Message::Error(_))));
    }

    #[test]
    fn pointer_events_do_not_break_the_window() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new();
        let mut w = AnalysisWindow::default();
        frame(&mut w, &ctx, &mut ed, input(), true);
        let mut i = input();
        i.events.push(Event::PointerMoved(Pos2::new(600.0, 300.0)));
        i.events.push(Event::PointerButton {
            pos: Pos2::new(600.0, 300.0),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        frame(&mut w, &ctx, &mut ed, i, true);
    }
}
