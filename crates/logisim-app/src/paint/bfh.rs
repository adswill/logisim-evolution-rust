//! Painters for the BFH library (binary → BCD converter, BCD → 7-segment decoder).

use super::block::{inside_text, port_at, port_label, rect_poly, Rect4};
use super::{port_bounds, union, Gfx, PartPainter, PartView};
use eframe::egui::Align2;

pub fn painter(part: &str) -> Option<&'static dyn PartPainter> {
    Some(match part {
        "Binary_to_BCD_converter" => &BinToBcdPainter,
        "BCD_to_7_Segment_decoder" => &BcdToSegPainter,
        _ => return None,
    })
}

struct BinToBcdPainter;
impl BinToBcdPainter {
    fn rect(v: &PartView) -> Rect4 {
        let max_x = v
            .ports
            .iter()
            .map(|p| p.offset.x as f32)
            .fold(0.0, f32::max);
        (-30.0, -20.0, max_x + 30.0, 20.0)
    }
}
impl PartPainter for BinToBcdPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let r = Self::rect(v);
        g.poly(&rect_poly(r), true, Some(th.fill), th.stroke);
        g.text(
            ((r.0 + r.2) / 2.0, 0.0),
            Align2::CENTER_CENTER,
            "binary → BCD",
            10.0,
            th.text,
        );
        port_label(g, v, 0, r, "bin");
        let digits = v.ports.len().saturating_sub(1);
        for i in 1..=digits {
            let weight = 10u64.pow((i - 1) as u32);
            port_label(g, v, i, r, &weight.to_string());
        }
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union(Self::rect(v), port_bounds(v, 0.0))
    }
}

struct BcdToSegPainter;
const BODY: Rect4 = (0.0, 0.0, 40.0, 70.0);
impl PartPainter for BcdToSegPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        g.poly(&rect_poly(BODY), true, Some(th.fill), th.stroke);
        g.text(
            (20.0, 35.0),
            Align2::CENTER_CENTER,
            "BCD\n→ 7seg",
            9.0,
            th.text,
        );
        for i in 0..7 {
            if let Some(p) = port_at(v, i) {
                let inward = if p.1 <= 0.0 { (0.0, 1.0) } else { (0.0, -1.0) };
                inside_text(
                    g,
                    p,
                    inward,
                    &((b'a' + i as u8) as char).to_string(),
                    7.0,
                    th.dim,
                );
            }
        }
        if let Some(p) = port_at(v, 7) {
            g.line(p, (p.0, BODY.3), v.port_color(th, 7));
        }
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union(BODY, port_bounds(v, 0.0))
    }
}
