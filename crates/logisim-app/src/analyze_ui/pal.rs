//! Colours and small drawing helpers for the analysis window. Colour is never the only carrier of
//! meaning: every table cell and map cell also shows its value as a character, and map groups
//! carry letters.

use eframe::egui::{Color32, FontId, Response};
use logisim_analyze::table::Entry;

#[derive(Clone, Copy)]
pub struct Pal {
    pub dark: bool,
    pub text: Color32,
    pub dim: Color32,
    pub grid: Color32,
    pub header_bg: Color32,
    pub accent: Color32,
    pub error: Color32,
    pub ok: Color32,
    /// Outline colours for K-map groups.
    pub groups: [Color32; 6],
}

impl Pal {
    pub fn new(dark: bool) -> Pal {
        if dark {
            Pal {
                dark,
                text: Color32::from_rgb(232, 234, 240),
                dim: Color32::from_rgb(160, 166, 178),
                grid: Color32::from_rgb(70, 75, 88),
                header_bg: Color32::from_rgb(40, 44, 54),
                accent: Color32::from_rgb(110, 175, 255),
                error: Color32::from_rgb(255, 128, 128),
                ok: Color32::from_rgb(110, 225, 150),
                groups: [
                    Color32::from_rgb(255, 170, 60),
                    Color32::from_rgb(90, 200, 250),
                    Color32::from_rgb(240, 110, 190),
                    Color32::from_rgb(150, 230, 90),
                    Color32::from_rgb(190, 150, 255),
                    Color32::from_rgb(255, 225, 90),
                ],
            }
        } else {
            Pal {
                dark,
                text: Color32::from_rgb(28, 30, 36),
                dim: Color32::from_rgb(88, 94, 108),
                grid: Color32::from_rgb(190, 195, 205),
                header_bg: Color32::from_rgb(232, 235, 241),
                accent: Color32::from_rgb(0, 90, 200),
                error: Color32::from_rgb(190, 30, 30),
                ok: Color32::from_rgb(20, 120, 60),
                groups: [
                    Color32::from_rgb(200, 100, 0),
                    Color32::from_rgb(0, 120, 180),
                    Color32::from_rgb(190, 40, 130),
                    Color32::from_rgb(60, 140, 20),
                    Color32::from_rgb(110, 70, 200),
                    Color32::from_rgb(150, 120, 0),
                ],
            }
        }
    }

    /// Background of a cell holding `e`.
    pub fn entry_fill(&self, e: Entry) -> Color32 {
        match (self.dark, e) {
            (true, Entry::Zero) => Color32::from_rgb(44, 48, 58),
            (true, Entry::One) => Color32::from_rgb(28, 98, 62),
            (true, Entry::DontCare) => Color32::from_rgb(42, 62, 108),
            (true, Entry::BusError) => Color32::from_rgb(130, 40, 40),
            (false, Entry::Zero) => Color32::from_rgb(236, 238, 243),
            (false, Entry::One) => Color32::from_rgb(190, 236, 206),
            (false, Entry::DontCare) => Color32::from_rgb(200, 216, 250),
            (false, Entry::BusError) => Color32::from_rgb(250, 200, 200),
        }
    }

    /// Text colour on top of [`Self::entry_fill`].
    pub fn entry_text(&self, e: Entry) -> Color32 {
        let fill = self.entry_fill(e);
        if contrast(self.text, fill) >= 4.5 {
            self.text
        } else if self.dark {
            Color32::WHITE
        } else {
            Color32::BLACK
        }
    }
}

fn lin(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance.
pub fn luminance(c: Color32) -> f32 {
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// WCAG contrast ratio (1 to 21).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Give a custom-drawn widget a spoken label for screen readers.
pub fn describe(r: Response, label: impl Fn() -> String) -> Response {
    r.widget_info(|| {
        eframe::egui::WidgetInfo::labeled(eframe::egui::accesskit::Role::Button, true, label())
    });
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_of_black_on_white_is_21() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 0.001);
    }

    #[test]
    fn cell_text_is_readable_on_every_fill_in_both_themes() {
        for dark in [true, false] {
            let p = Pal::new(dark);
            for e in [Entry::Zero, Entry::One, Entry::DontCare, Entry::BusError] {
                let c = contrast(p.entry_text(e), p.entry_fill(e));
                assert!(c >= 4.5, "dark={dark} {e:?}: contrast {c:.2}");
            }
        }
    }

    #[test]
    fn colours_meet_contrast_against_the_window_background() {
        // egui's window fills: dark 27,27,27 and light 248,248,248
        for (dark, bg) in [
            (true, Color32::from_rgb(27, 27, 27)),
            (false, Color32::from_rgb(248, 248, 248)),
        ] {
            let p = Pal::new(dark);
            for (name, c) in [
                ("text", p.text),
                ("dim", p.dim),
                ("accent", p.accent),
                ("error", p.error),
                ("ok", p.ok),
            ] {
                assert!(
                    contrast(c, bg) >= 4.5,
                    "dark={dark} {name}: {:.2}",
                    contrast(c, bg)
                );
            }
            for (i, c) in p.groups.iter().enumerate() {
                assert!(
                    contrast(*c, bg) >= 3.0,
                    "dark={dark} group {i}: {:.2}",
                    contrast(*c, bg)
                );
            }
        }
    }

    #[test]
    fn group_colours_are_distinct() {
        for dark in [true, false] {
            let g = Pal::new(dark).groups;
            for i in 0..6 {
                for j in i + 1..6 {
                    assert_ne!(g[i], g[j]);
                }
            }
        }
    }
}
