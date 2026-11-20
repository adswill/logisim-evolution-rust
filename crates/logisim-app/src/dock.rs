//! Dockable panel layout (egui_dock): which panels exist, the default arrangement, saving and
//! restoring it. Panels can be dragged, split, tabbed and resized; "Reset layout" restores this.

use crate::app::App;
use eframe::egui;
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelKind {
    Toolbox,
    Circuits,
    Canvas,
    Attributes,
}

impl PanelKind {
    pub const ALL: [PanelKind; 4] = [
        PanelKind::Toolbox,
        PanelKind::Circuits,
        PanelKind::Canvas,
        PanelKind::Attributes,
    ];
    pub fn title(self) -> &'static str {
        match self {
            PanelKind::Toolbox => "Parts",
            PanelKind::Circuits => "Circuits",
            PanelKind::Canvas => "Canvas",
            PanelKind::Attributes => "Attributes",
        }
    }
}

/// Parts and circuits on the left, canvas in the middle, attributes on the right.
pub fn default_layout() -> DockState<PanelKind> {
    let mut dock = DockState::new(vec![PanelKind::Canvas]);
    let tree = dock.main_surface_mut();
    let [canvas, _right] = tree.split_right(NodeIndex::root(), 0.78, vec![PanelKind::Attributes]);
    let [_canvas, left] = tree.split_left(canvas, 0.22, vec![PanelKind::Toolbox]);
    tree.split_below(left, 0.70, vec![PanelKind::Circuits]);
    dock
}

pub fn to_text(dock: &DockState<PanelKind>) -> Option<String> {
    ron::to_string(dock).ok()
}

/// Restore a saved layout; anything damaged or missing a required panel gives the default.
pub fn from_text(text: &str) -> DockState<PanelKind> {
    match ron::from_str::<DockState<PanelKind>>(text) {
        Ok(d) if PanelKind::ALL.iter().all(|k| d.find_tab(k).is_some()) => d,
        _ => default_layout(),
    }
}

struct Viewer<'a> {
    app: &'a mut App,
}

impl TabViewer for Viewer<'_> {
    type Tab = PanelKind;

    fn id(&mut self, tab: &mut PanelKind) -> egui::Id {
        egui::Id::unique(("panel", *tab))
    }
    fn title(&mut self, tab: &mut PanelKind) -> egui::WidgetText {
        tab.title().into()
    }
    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut PanelKind) {
        match tab {
            PanelKind::Toolbox => self.app.toolbox(ui),
            PanelKind::Circuits => self.app.circuits_panel(ui),
            PanelKind::Canvas => {
                self.app.circuit_tabs(ui);
                self.app.canvas(ui);
            }
            PanelKind::Attributes => self.app.attributes(ui),
        }
    }
    /// Panels can be moved but not closed, so the editor can never end up without its canvas.
    fn is_closeable(&self, _tab: &PanelKind) -> bool {
        false
    }
    fn scroll_bars(&self, _tab: &PanelKind) -> [bool; 2] {
        [false, false]
    }
}

impl App {
    pub(crate) fn dock_ui(&mut self, ui: &mut egui::Ui) {
        if std::mem::take(&mut self.reset_layout) {
            self.dock = default_layout();
            self.prefs.layout = None;
        }
        let mut dock = std::mem::replace(&mut self.dock, DockState::new(vec![]));
        let style = Style::from_egui(ui.style());
        DockArea::new(&mut dock)
            .style(style)
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .show_add_buttons(false)
            .show_inside(ui, &mut Viewer { app: self });
        self.dock = dock;
        // save the layout now and then (cheap compare), not every frame
        self.frame += 1;
        if self.frame % 30 == 0 {
            if let Some(t) = to_text(&self.dock) {
                if self.prefs.layout.as_deref() != Some(t.as_str()) {
                    self.prefs.layout = Some(t);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_has_every_panel_once() {
        let d = default_layout();
        for k in PanelKind::ALL {
            assert!(d.find_tab(&k).is_some(), "{k:?} missing");
        }
        assert_eq!(d.iter_all_tabs().count(), PanelKind::ALL.len());
    }

    #[test]
    fn layout_round_trips_through_text() {
        let text = to_text(&default_layout()).expect("serialises");
        let back = from_text(&text);
        assert_eq!(to_text(&back).unwrap(), text);
    }

    #[test]
    fn damaged_or_incomplete_layouts_fall_back_to_default() {
        let def = to_text(&default_layout()).unwrap();
        assert_eq!(to_text(&from_text("nonsense")).unwrap(), def);
        let only_canvas = to_text(&DockState::new(vec![PanelKind::Canvas])).unwrap();
        assert_eq!(
            to_text(&from_text(&only_canvas)).unwrap(),
            def,
            "a layout without the other panels is replaced"
        );
    }
}
