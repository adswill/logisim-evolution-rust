//! Painter for the Base library: the Text annotation.

use super::{Gfx, PartPainter, PartView};
use eframe::egui::{Align2, Color32};

pub fn painter(part: &str) -> Option<&'static dyn PartPainter> {
    match part {
        "Text" => Some(&TextPainter),
        _ => None,
    }
}

struct TextPainter;

fn parse_color(s: &str) -> Option<Color32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

impl TextPainter {
    fn size(v: &PartView) -> f32 {
        v.int("font_size", 12).clamp(4, 200) as f32
    }
    fn anchor(v: &PartView) -> Align2 {
        let h = match v.string("halign").as_str() {
            "left" => 0,
            "right" => 2,
            _ => 1,
        };
        let vv = match v.string("valign").as_str() {
            "top" => 0,
            "center" => 1,
            _ => 2, // base / bottom: text sits above the anchor
        };
        [
            [Align2::LEFT_TOP, Align2::CENTER_TOP, Align2::RIGHT_TOP],
            [
                Align2::LEFT_CENTER,
                Align2::CENTER_CENTER,
                Align2::RIGHT_CENTER,
            ],
            [
                Align2::LEFT_BOTTOM,
                Align2::CENTER_BOTTOM,
                Align2::RIGHT_BOTTOM,
            ],
        ][vv][h]
    }
}

impl PartPainter for TextPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let text = v.string("text");
        let text = if text.is_empty() {
            "Text".to_string()
        } else {
            text
        };
        let color = match v.string("color").as_str() {
            // the Logisim default is black, which would vanish on the dark theme
            "" | "#000000" => g.theme.text,
            c => parse_color(c).unwrap_or(g.theme.text),
        };
        g.text(
            (0.0, 0.0),
            Self::anchor(v),
            &text,
            Self::size(v) * 0.85,
            color,
        );
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let text = v.string("text");
        let lines = text.lines().count().max(1) as f32;
        let cols = text
            .lines()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(4)
            .max(4) as f32;
        let s = Self::size(v);
        let (w, h) = (cols * s * 0.55, lines * s * 1.1);
        let x = match v.string("halign").as_str() {
            "left" => 0.0,
            "right" => -w,
            _ => -w / 2.0,
        };
        let y = match v.string("valign").as_str() {
            "top" => 0.0,
            "center" => -h / 2.0,
            _ => -h,
        };
        (x - 2.0, y - 2.0, x + w + 2.0, y + h + 2.0)
    }
}
