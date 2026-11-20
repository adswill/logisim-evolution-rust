//! Plug-in windows ("App windows"): analysis, timing diagram, hex editor, statistics, ...
//! A window implements [`AppWindow`] and is registered with ONE line in [`registered_windows`].
//! The editor then lists it in the Window menu and the command palette, opens it in a floating
//! egui window and keeps it open across frames.

use crate::model::Editor;
use crate::sim::Sim;
use eframe::egui;
use logisim_format::ProjectFile;
use std::collections::HashSet;

/// Which group of the Window menu a window is listed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowGroup {
    Project,
    Simulate,
    Tools,
}

impl WindowGroup {
    pub const ALL: [WindowGroup; 3] = [
        WindowGroup::Project,
        WindowGroup::Simulate,
        WindowGroup::Tools,
    ];
    pub fn label(self) -> &'static str {
        match self {
            WindowGroup::Project => "Project",
            WindowGroup::Simulate => "Simulate",
            WindowGroup::Tools => "Tools",
        }
    }
}

/// What a window may look at and change. Edits to the circuit should go through `editor`'s
/// methods (undoable); the simulation is the live one of the open circuit.
pub struct WindowEnv<'a> {
    pub editor: &'a mut Editor,
    /// Live simulation of the open circuit (`None` when simulation is off or failed to build).
    pub sim: Option<&'a mut Sim>,
    /// Message shown in the status bar.
    pub status: &'a mut String,
    /// For `request_repaint*` and other egui context calls.
    pub ctx: &'a egui::Context,
    /// Frame counter (same value for every window within one frame).
    pub frame: u64,
}

pub trait AppWindow {
    /// Stable unique id (used for the open/closed state), e.g. `"analysis"`.
    fn id(&self) -> &'static str;
    /// Title of the window and its menu entry.
    fn title(&self) -> &str;
    fn group(&self) -> WindowGroup {
        WindowGroup::Tools
    }
    /// One-line help for the palette / tooltip.
    fn description(&self) -> &str {
        ""
    }
    /// Draw the contents. Called every frame while the window is open.
    fn show(&mut self, ui: &mut egui::Ui, env: &mut WindowEnv);
    /// Called every frame while open, before `show` (e.g. to sample the simulation for a timing
    /// diagram). Default: nothing.
    fn update(&mut self, _env: &mut WindowEnv) {}
    /// Default size in logical pixels.
    fn default_size(&self) -> [f32; 2] {
        [520.0, 420.0]
    }
}

/// ONE LINE PER WINDOW: add `reg.push(Box::new(my_module::MyWindow::default()));` here.
pub fn registered_windows() -> Vec<Box<dyn AppWindow>> {
    let mut reg: Vec<Box<dyn AppWindow>> = vec![];
    reg.push(Box::new(ProjectInfoWindow));
    reg.push(Box::new(crate::analyze_ui::AnalysisWindow::default()));
    crate::log_ui::register(&mut reg);
    crate::tools::register(&mut reg);
    reg
}

/// Open/closed state for all registered windows.
pub struct Windows {
    pub list: Vec<Box<dyn AppWindow>>,
    pub open: HashSet<&'static str>,
}

impl Default for Windows {
    fn default() -> Self {
        Windows {
            list: registered_windows(),
            open: HashSet::new(),
        }
    }
}

impl Windows {
    pub fn toggle(&mut self, id: &'static str) {
        if !self.open.remove(id) {
            self.open.insert(id);
        }
    }
    pub fn open(&mut self, id: &str) {
        if let Some(w) = self.list.iter().find(|w| w.id() == id) {
            self.open.insert(w.id());
        }
    }

    /// Draw every open window (floating, closable).
    pub fn show(&mut self, ctx: &egui::Context, env: &mut WindowEnv) {
        let mut closed = vec![];
        for w in self.list.iter_mut().filter(|w| self.open.contains(w.id())) {
            w.update(env);
            let mut open = true;
            let [dw, dh] = w.default_size();
            egui::Window::new(w.title().to_string())
                .id(egui::Id::unique(("app_window", w.id())))
                .open(&mut open)
                .default_size([dw, dh])
                .show(ctx, |ui| w.show(ui, env));
            if !open {
                closed.push(w.id());
            }
        }
        for id in closed {
            self.open.remove(id);
        }
    }
}

// ---- a small built-in example window --------------------------------------------------------

/// Counts for the "Project info" window.
#[derive(Debug, PartialEq, Eq)]
pub struct ProjectStats {
    pub circuits: usize,
    pub parts: usize,
    pub wires: usize,
    pub subcircuit_instances: usize,
}

impl ProjectStats {
    pub fn of(p: &ProjectFile) -> Self {
        ProjectStats {
            circuits: p.circuits.len(),
            parts: p.circuits.iter().map(|c| c.components.len()).sum(),
            wires: p.circuits.iter().map(|c| c.wires.len()).sum(),
            subcircuit_instances: p
                .circuits
                .iter()
                .flat_map(|c| &c.components)
                .filter(|d| d.library == logisim_format::SUBCIRCUIT_LIBRARY)
                .count(),
        }
    }
}

struct ProjectInfoWindow;

impl AppWindow for ProjectInfoWindow {
    fn id(&self) -> &'static str {
        "project_info"
    }
    fn title(&self) -> &str {
        "Project info"
    }
    fn group(&self) -> WindowGroup {
        WindowGroup::Project
    }
    fn description(&self) -> &str {
        "How many circuits, parts and wires the project has."
    }
    fn default_size(&self) -> [f32; 2] {
        [320.0, 200.0]
    }
    fn show(&mut self, ui: &mut egui::Ui, env: &mut WindowEnv) {
        let st = ProjectStats::of(&env.editor.project);
        egui::Grid::new("project_info_grid")
            .num_columns(2)
            .show(ui, |ui| {
                for (k, v) in [
                    ("Circuits", st.circuits),
                    ("Parts", st.parts),
                    ("Wires", st.wires),
                    ("Circuit instances", st.subcircuit_instances),
                ] {
                    ui.label(k);
                    ui.monospace(v.to_string());
                    ui.end_row();
                }
            });
        ui.separator();
        ui.label(format!("Project: {}", env.editor.project.metadata.name));
        ui.label(format!("Open circuit: {}", env.editor.current));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_windows_have_unique_ids_and_titles() {
        let list = registered_windows();
        let ids: HashSet<_> = list.iter().map(|w| w.id()).collect();
        assert_eq!(ids.len(), list.len(), "duplicate window id");
        assert!(list
            .iter()
            .all(|w| !w.title().is_empty() && !w.id().is_empty()));
    }

    #[test]
    fn open_state_toggles() {
        let mut w = Windows::default();
        assert!(w.open.is_empty());
        w.toggle("project_info");
        assert!(w.open.contains("project_info"));
        w.toggle("project_info");
        assert!(w.open.is_empty());
        w.open("does_not_exist");
        assert!(w.open.is_empty());
    }

    #[test]
    fn project_stats_count_parts_wires_and_instances() {
        let e = crate::examples::half_adder();
        let st = ProjectStats::of(&e.project);
        assert_eq!(
            (st.circuits, st.parts, st.wires, st.subcircuit_instances),
            (1, 6, 10, 0)
        );
    }
}
