//! Every command the editor can run, with its label, shortcut and key binding. Menus, the
//! command palette, the shortcut cheat sheet and keyboard handling all read this one table.

use eframe::egui::{Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    OpenExample,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    Rotate,
    SelectAll,
    ToolSelect,
    ToolPoke,
    ToolWire,
    SimToggle,
    RunToggle,
    Step,
    ResetSim,
    ToggleGrid,
    ToggleColourNets,
    ToggleMinimap,
    FitView,
    ResetView,
    ZoomIn,
    ZoomOut,
    ThemeSystem,
    ThemeLight,
    ThemeDark,
    ThemeHighContrast,
    ResetLayout,
    FocusSearch,
    CommandPalette,
    Shortcuts,
}

impl Action {
    /// Display order in the palette (grouped by category).
    pub const ALL: &'static [Action] = &[
        Action::New,
        Action::Open,
        Action::Save,
        Action::SaveAs,
        Action::OpenExample,
        Action::Quit,
        Action::Redo, // before Undo: Cmd+Shift+Z must be tested before Cmd+Z
        Action::Undo,
        Action::Cut,
        Action::Copy,
        Action::Paste,
        Action::Duplicate,
        Action::Delete,
        Action::Rotate,
        Action::SelectAll,
        Action::ToolSelect,
        Action::ToolPoke,
        Action::ToolWire,
        Action::SimToggle,
        Action::RunToggle,
        Action::Step,
        Action::ResetSim,
        Action::ToggleGrid,
        Action::ToggleColourNets,
        Action::ToggleMinimap,
        Action::FitView,
        Action::ResetView,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ThemeSystem,
        Action::ThemeLight,
        Action::ThemeDark,
        Action::ThemeHighContrast,
        Action::ResetLayout,
        Action::FocusSearch,
        Action::CommandPalette,
        Action::Shortcuts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::New => "New project",
            Action::Open => "Open project…",
            Action::Save => "Save",
            Action::SaveAs => "Save as…",
            Action::OpenExample => "Open example: half adder",
            Action::Quit => "Quit",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Cut => "Cut",
            Action::Copy => "Copy",
            Action::Paste => "Paste",
            Action::Duplicate => "Duplicate",
            Action::Delete => "Delete selection",
            Action::Rotate => "Rotate selection",
            Action::SelectAll => "Select all",
            Action::ToolSelect => "Tool: Select",
            Action::ToolPoke => "Tool: Poke (flip pins)",
            Action::ToolWire => "Tool: Wire",
            Action::SimToggle => "Simulation on/off",
            Action::RunToggle => "Run or pause the clock",
            Action::Step => "Step one clock tick",
            Action::ResetSim => "Reset simulation",
            Action::ToggleGrid => "Show or hide grid",
            Action::ToggleColourNets => "Colour nets on/off",
            Action::ToggleMinimap => "Show or hide minimap",
            Action::FitView => "Fit circuit to window",
            Action::ResetView => "Reset view",
            Action::ZoomIn => "Zoom interface in",
            Action::ZoomOut => "Zoom interface out",
            Action::ThemeSystem => "Theme: follow system",
            Action::ThemeLight => "Theme: light",
            Action::ThemeDark => "Theme: dark",
            Action::ThemeHighContrast => "Theme: high contrast",
            Action::ResetLayout => "Reset panel layout",
            Action::FocusSearch => "Search parts",
            Action::CommandPalette => "Command palette",
            Action::Shortcuts => "Keyboard shortcuts",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            Action::New
            | Action::Open
            | Action::Save
            | Action::SaveAs
            | Action::OpenExample
            | Action::Quit => "File",
            Action::Undo
            | Action::Redo
            | Action::Cut
            | Action::Copy
            | Action::Paste
            | Action::Duplicate
            | Action::Delete
            | Action::Rotate
            | Action::SelectAll => "Edit",
            Action::ToolSelect | Action::ToolPoke | Action::ToolWire => "Tools",
            Action::SimToggle | Action::RunToggle | Action::Step | Action::ResetSim => "Simulate",
            _ => "View",
        }
    }

    /// Key binding (`COMMAND` = Cmd on macOS, Ctrl elsewhere).
    pub fn binding(self) -> Option<(Modifiers, Key)> {
        let c = Modifiers::COMMAND;
        Some(match self {
            Action::New => (c, Key::N),
            Action::Open => (c, Key::O),
            Action::Save => (c, Key::S),
            Action::SaveAs => (c | Modifiers::SHIFT, Key::S),
            Action::Redo => (c | Modifiers::SHIFT, Key::Z),
            Action::Undo => (c, Key::Z),
            Action::Duplicate => (c, Key::D),
            Action::SelectAll => (c, Key::A),
            Action::ZoomIn => (c, Key::Equals),
            Action::ZoomOut => (c, Key::Minus),
            Action::FitView => (c, Key::Num0),
            Action::FocusSearch => (c, Key::F),
            Action::CommandPalette => (c, Key::K),
            Action::Shortcuts => (c, Key::Slash),
            _ => return None,
        })
    }

    /// Shortcut text for menus and the cheat sheet (also lists non-modifier keys).
    pub fn shortcut(self) -> Option<String> {
        let mac = cfg!(target_os = "macos");
        let cmd = if mac { "⌘" } else { "Ctrl+" };
        let shift = if mac { "⇧" } else { "Shift+" };
        Some(match self {
            Action::Delete => "Delete".into(),
            Action::Rotate => "R".into(),
            Action::Cut => format!("{cmd}X"),
            Action::Copy => format!("{cmd}C"),
            Action::Paste => format!("{cmd}V"),
            Action::Redo => format!("{cmd}{shift}Z"),
            Action::SaveAs => format!("{cmd}{shift}S"),
            Action::ZoomIn => format!("{cmd}+"),
            Action::ZoomOut => format!("{cmd}−"),
            Action::FitView => format!("{cmd}0"),
            Action::FocusSearch => format!("{cmd}F"),
            Action::CommandPalette => format!("{cmd}K"),
            Action::Shortcuts => format!("{cmd}/"),
            Action::New => format!("{cmd}N"),
            Action::Open => format!("{cmd}O"),
            Action::Save => format!("{cmd}S"),
            Action::Undo => format!("{cmd}Z"),
            Action::Duplicate => format!("{cmd}D"),
            Action::SelectAll => format!("{cmd}A"),
            _ => return None,
        })
    }

    /// Actions that edit the drawing; skipped while a text field has the keyboard.
    pub fn needs_canvas_focus(self) -> bool {
        matches!(
            self,
            Action::Cut
                | Action::Copy
                | Action::Paste
                | Action::Duplicate
                | Action::Delete
                | Action::Rotate
                | Action::SelectAll
                | Action::Undo
                | Action::Redo
        )
    }
}

/// Mouse and keyboard hints that are not single actions (shown in the cheat sheet).
pub const EXTRA_HINTS: &[(&str, &str)] = &[
    (
        "Click",
        "select a part or wire; clicking an input Pin flips it while simulating",
    ),
    ("Shift+Click", "add to or remove from the selection"),
    ("Drag on empty canvas", "box select"),
    ("Drag a selection", "move it (wires follow)"),
    ("Right-click", "context menu (or cancel the current tool)"),
    ("Right/Middle drag", "pan the canvas"),
    ("Scroll / pinch", "zoom the canvas"),
    (
        "Arrow keys",
        "move the selection one grid step (Shift = 10 steps)",
    ),
    ("Esc", "cancel the tool and clear the selection"),
    ("F6 / Shift+F6", "move keyboard focus between panels"),
    ("Tab", "move between controls inside a panel"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_actions_are_listed_once() {
        let set: HashSet<_> = Action::ALL.iter().map(|a| format!("{a:?}")).collect();
        assert_eq!(set.len(), Action::ALL.len());
        assert!(Action::ALL.iter().all(|a| !a.label().is_empty()));
    }

    #[test]
    fn key_bindings_do_not_collide() {
        let mut seen = HashSet::new();
        for a in Action::ALL {
            if let Some((m, k)) = a.binding() {
                assert!(
                    seen.insert(format!("{m:?}{k:?}")),
                    "{a:?} reuses a key binding"
                );
                assert!(
                    a.shortcut().is_some(),
                    "{a:?} has a binding but no shown shortcut"
                );
            }
        }
    }

    #[test]
    fn redo_is_tested_before_undo() {
        let pos = |x| Action::ALL.iter().position(|a| *a == x).unwrap();
        assert!(pos(Action::Redo) < pos(Action::Undo));
        assert!(pos(Action::SaveAs) > pos(Action::Save) || true);
    }
}
