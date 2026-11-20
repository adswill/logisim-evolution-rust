//! `AppWindow` adapters for the tools in this folder. [`register`] is the single entry point
//! the window registry calls.

use super::export::{self, ExportOptions};
use super::hexedit::{HexEditor, HexEvent};
use super::print;
use super::stats::StatsWindow;
use crate::model::Sel;
use crate::windows::{AppWindow, WindowEnv, WindowGroup};
use eframe::egui;

pub fn register(reg: &mut Vec<Box<dyn AppWindow>>) {
    reg.push(Box::new(StatsTool::default()));
    reg.push(Box::new(HexTool::default()));
    reg.push(Box::new(ExportTool::default()));
}

// ---- statistics -------------------------------------------------------------------------------

#[derive(Default)]
struct StatsTool {
    win: Option<(StatsWindow, u64, String)>,
}

impl AppWindow for StatsTool {
    fn id(&self) -> &'static str {
        "statistics"
    }
    fn title(&self) -> &str {
        "Statistics"
    }
    fn group(&self) -> WindowGroup {
        WindowGroup::Project
    }
    fn description(&self) -> &str {
        "Count the components used by the open circuit, with and without subcircuits."
    }
    fn default_size(&self) -> [f32; 2] {
        [460.0, 380.0]
    }
    fn show(&mut self, ui: &mut egui::Ui, env: &mut WindowEnv) {
        let key = (env.editor.version, env.editor.current.clone());
        let stale = !matches!(&self.win, Some((_, v, c)) if *v == key.0 && *c == key.1);
        if stale {
            let w = StatsWindow::new(&env.editor.project, &key.1);
            self.win = Some((w, key.0, key.1));
        }
        if let Some((w, _, c)) = &mut self.win {
            ui.strong(format!("Circuit: {c}"));
            w.ui(ui);
        }
    }
}

// ---- hex editor -------------------------------------------------------------------------------

#[derive(Default)]
struct HexTool {
    /// (circuit, component index, editor)
    target: Option<(String, usize, HexEditor)>,
}

fn selected_memory(env: &WindowEnv) -> Option<usize> {
    let [Sel::Comp(i)] = env.editor.selection[..] else {
        return None;
    };
    let d = env.editor.circuit().components.get(i)?;
    (d.library == "Memory" && matches!(d.part.as_str(), "ROM" | "RAM")).then_some(i)
}

impl AppWindow for HexTool {
    fn id(&self) -> &'static str {
        "hex_editor"
    }
    fn title(&self) -> &str {
        "Memory contents"
    }
    fn group(&self) -> WindowGroup {
        WindowGroup::Tools
    }
    fn description(&self) -> &str {
        "Edit the contents of the selected RAM or ROM; import and export hex files."
    }
    fn default_size(&self) -> [f32; 2] {
        [640.0, 480.0]
    }
    fn show(&mut self, ui: &mut egui::Ui, env: &mut WindowEnv) {
        // follow the selection unless there are unapplied edits
        let dirty = self
            .target
            .as_ref()
            .is_some_and(|(_, _, e)| e.is_modified());
        if !dirty {
            if let Some(i) = selected_memory(env) {
                let cur = env.editor.current.clone();
                if !matches!(&self.target, Some((c, j, _)) if *c == cur && *j == i) {
                    match HexEditor::for_component(&env.editor.circuit().components[i]) {
                        Ok(ed) => self.target = Some((cur, i, ed)),
                        Err(e) => {
                            *env.status = e;
                            self.target = None;
                        }
                    }
                }
            }
        }
        let Some((circ, idx, ed)) = &mut self.target else {
            ui.label("Select a RAM or ROM in the circuit, then edit its contents here.");
            return;
        };
        ui.horizontal(|ui| {
            ui.strong(format!(
                "{} in {}",
                env.editor
                    .project
                    .circuit(circ)
                    .and_then(|c| c.components.get(*idx))
                    .map_or("?", |d| d.part.as_str()),
                circ
            ));
            if ed.is_modified() {
                ui.weak("(not applied yet)");
            }
        });
        if ed.ui(ui) == HexEvent::Apply {
            if *circ != env.editor.current {
                *env.status = format!("Open circuit '{circ}' to apply these contents.");
            } else if let Some(mut def) = env.editor.circuit().components.get(*idx).cloned() {
                ed.apply_to(&mut def);
                env.editor.replace_component(*idx, def);
                ed.mark_applied();
                *env.status = "Memory contents updated.".into();
            }
        }
    }
}

// ---- export / print ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Png,
    Svg,
}

struct ExportTool {
    format: Format,
    scale: f32,
    dark: bool,
    background: bool,
    landscape: bool,
    all_circuits: bool,
    message: Option<(bool, String)>,
}

impl Default for ExportTool {
    fn default() -> Self {
        ExportTool {
            format: Format::Png,
            scale: 2.0,
            dark: false,
            background: true,
            landscape: true,
            all_circuits: false,
            message: None,
        }
    }
}

impl AppWindow for ExportTool {
    fn id(&self) -> &'static str {
        "export_image"
    }
    fn title(&self) -> &str {
        "Export image / Print"
    }
    fn group(&self) -> WindowGroup {
        WindowGroup::Project
    }
    fn description(&self) -> &str {
        "Save the open circuit as a PNG or SVG picture, or print it."
    }
    fn default_size(&self) -> [f32; 2] {
        [360.0, 300.0]
    }
    fn show(&mut self, ui: &mut egui::Ui, env: &mut WindowEnv) {
        let name = env.editor.current.clone();
        ui.strong(format!("Circuit: {name}"));
        ui.horizontal(|ui| {
            ui.label("Format:");
            ui.radio_value(&mut self.format, Format::Png, "PNG");
            ui.radio_value(&mut self.format, Format::Svg, "SVG (vector)");
        });
        if self.format == Format::Png {
            ui.horizontal(|ui| {
                ui.label("Scale:");
                ui.add(egui::Slider::new(&mut self.scale, 1.0..=6.0).suffix("×"));
            });
        }
        ui.checkbox(&mut self.dark, "Dark colours");
        ui.checkbox(&mut self.background, "Paint background");
        let opts = ExportOptions {
            scale: if self.format == Format::Png {
                self.scale
            } else {
                1.0
            },
            dark: self.dark,
            background: self.background,
            ..Default::default()
        };
        ui.add_space(4.0);
        if ui.button("Save image…").clicked() {
            let (ext, label) = match self.format {
                Format::Png => ("png", "PNG image"),
                Format::Svg => ("svg", "SVG image"),
            };
            if let Some(path) = rfd::FileDialog::new()
                .add_filter(label, &[ext])
                .set_file_name(format!("{name}.{ext}"))
                .save_file()
            {
                let res = match self.format {
                    Format::Png => export::export_png(&env.editor.project, &name, &opts),
                    Format::Svg => export::export_svg(&env.editor.project, &name, &opts)
                        .map(String::into_bytes),
                };
                self.message = Some(
                    match res.and_then(|b| {
                        std::fs::write(&path, b).map_err(|e| format!("Could not save: {e}"))
                    }) {
                        Ok(()) => (false, format!("Saved {}", path.display())),
                        Err(e) => (true, e),
                    },
                );
            }
        }
        ui.separator();
        ui.checkbox(&mut self.all_circuits, "Print every circuit (one per page)");
        ui.checkbox(&mut self.landscape, "Landscape");
        if ui
            .button("Print…")
            .on_hover_text("Opens a print-ready page in your browser")
            .clicked()
        {
            let list: Vec<String> = if self.all_circuits {
                env.editor
                    .project
                    .circuits
                    .iter()
                    .map(|c| c.name.clone())
                    .collect()
            } else {
                vec![name]
            };
            self.message = Some(
                match print::print_html(&env.editor.project, &list, self.landscape)
                    .and_then(|h| print::open_for_printing(&h))
                {
                    Ok(_) => (false, "Opened the print page in your browser.".into()),
                    Err(e) => (true, e),
                },
            );
        }
        if let Some((err, m)) = &self.message {
            let c = if *err {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            };
            ui.colored_label(c, m);
        }
    }
}
