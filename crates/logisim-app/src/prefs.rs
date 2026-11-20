//! User preferences, saved as RON in the platform config directory. Loading never fails:
//! a missing or damaged file just gives the defaults.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeChoice {
    /// Follow the operating system (light or dark).
    #[default]
    System,
    Light,
    Dark,
    HighContrast,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 4] = [
        ThemeChoice::System,
        ThemeChoice::Light,
        ThemeChoice::Dark,
        ThemeChoice::HighContrast,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "Follow system",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
            ThemeChoice::HighContrast => "High contrast",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemeChoice,
    /// Whole-UI zoom (Cmd +/-), 0.7..=2.0.
    pub ui_scale: f32,
    /// Extra text size multiplier on top of the UI scale, 0.8..=1.6.
    pub font_scale: f32,
    pub show_grid: bool,
    pub colour_nets: bool,
    pub show_minimap: bool,
    pub hz: f32,
    /// "Library/Part" ids.
    pub favourites: Vec<String>,
    pub recent_parts: Vec<String>,
    pub recent_files: Vec<String>,
    /// Dock layout as RON (see `dock`), `None` = default layout.
    pub layout: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: ThemeChoice::System,
            ui_scale: 1.0,
            font_scale: 1.0,
            show_grid: true,
            colour_nets: true,
            show_minimap: false,
            hz: 2.0,
            favourites: vec![],
            recent_parts: vec![],
            recent_files: vec![],
            layout: None,
        }
    }
}

pub const MAX_RECENT_PARTS: usize = 8;
pub const MAX_RECENT_FILES: usize = 8;

impl Prefs {
    pub fn parse(text: &str) -> Prefs {
        let mut p: Prefs = ron::from_str(text).unwrap_or_default();
        p.ui_scale = p.ui_scale.clamp(0.7, 2.0);
        p.font_scale = p.font_scale.clamp(0.8, 1.6);
        p.hz = p.hz.clamp(0.1, 60.0);
        p
    }

    pub fn to_text(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).unwrap_or_default()
    }

    /// `~/Library/Application Support/rcirc`, `$XDG_CONFIG_HOME/rcirc` or `%APPDATA%\rcirc`.
    pub fn dir() -> Option<PathBuf> {
        let env = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let base = if cfg!(target_os = "windows") {
            env("APPDATA")?
        } else if cfg!(target_os = "macos") {
            env("HOME")?.join("Library").join("Application Support")
        } else {
            env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?
        };
        Some(base.join("rcirc"))
    }

    pub fn load() -> Prefs {
        Prefs::dir()
            .and_then(|d| std::fs::read_to_string(d.join("prefs.ron")).ok())
            .map(|t| Prefs::parse(&t))
            .unwrap_or_default()
    }

    /// Best effort: a read-only config directory must never break the editor.
    pub fn save(&self) {
        if let Some(d) = Prefs::dir() {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("prefs.ron"), self.to_text());
        }
    }

    /// Move `id` to the front of a most-recently-used list.
    pub fn push_recent(list: &mut Vec<String>, id: &str, max: usize) {
        list.retain(|x| x != id);
        list.insert(0, id.to_string());
        list.truncate(max);
    }

    pub fn toggle_favourite(&mut self, id: &str) {
        if let Some(i) = self.favourites.iter().position(|x| x == id) {
            self.favourites.remove(i);
        } else {
            self.favourites.push(id.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_survives_garbage() {
        let mut p = Prefs {
            theme: ThemeChoice::HighContrast,
            ui_scale: 1.25,
            show_minimap: true,
            ..Prefs::default()
        };
        p.favourites.push("Gates/AND Gate".into());
        assert_eq!(Prefs::parse(&p.to_text()), p);
        assert_eq!(Prefs::parse("this is not ron"), Prefs::default());
        assert_eq!(Prefs::parse(""), Prefs::default());
    }

    #[test]
    fn old_files_with_missing_fields_still_load() {
        let p = Prefs::parse("(theme: Dark)");
        assert_eq!(p.theme, ThemeChoice::Dark);
        assert!(p.show_grid && p.colour_nets);
    }

    #[test]
    fn values_are_clamped() {
        let p = Prefs::parse("(ui_scale: 9.0, font_scale: 0.0, hz: 0.0)");
        assert_eq!((p.ui_scale, p.font_scale, p.hz), (2.0, 0.8, 0.1));
    }

    #[test]
    fn recent_lists_are_mru_and_bounded() {
        let mut l = vec![];
        for i in 0..12 {
            Prefs::push_recent(&mut l, &format!("p{i}"), 8);
        }
        assert_eq!(l.len(), 8);
        assert_eq!(l[0], "p11");
        Prefs::push_recent(&mut l, "p5", 8);
        assert_eq!(l[0], "p5");
        assert_eq!(l.iter().filter(|x| *x == "p5").count(), 1);
    }

    #[test]
    fn favourites_toggle() {
        let mut p = Prefs::default();
        p.toggle_favourite("Gates/AND Gate");
        assert_eq!(p.favourites, vec!["Gates/AND Gate"]);
        p.toggle_favourite("Gates/AND Gate");
        assert!(p.favourites.is_empty());
    }
}
