//! Painters for the Input/Output library. Indicators read their state from the live port values
//! (an LED lights when its input net is high), so they animate while simulating.

use super::block::{rect_poly, Rect4};
use super::{port_bounds, union, Gfx, PartPainter, PartView};
use eframe::egui::{Align2, Color32};
use logisim_core::Value;

pub fn painter(part: &str) -> Option<&'static dyn PartPainter> {
    Some(match part {
        "Button" => &ButtonPainter,
        "DIP Switch" => &DipPainter,
        "Joystick" => &JoystickPainter,
        "Keyboard" => &TerminalPainter { keyboard: true },
        "TTY" => &TerminalPainter { keyboard: false },
        "LED" => &LedPainter,
        "RGB LED" => &RgbLedPainter,
        "LED Bar" => &BarPainter,
        "7-Segment Display" => &SegmentPainter { hex: false },
        "Hex Digit Display" => &SegmentPainter { hex: true },
        "LED Matrix" => &MatrixPainter,
        _ => return None,
    })
}

const LIT: Color32 = Color32::from_rgb(235, 60, 60);

/// Bit `i` of the live value on port `p`: `Some(true/false)` or `None` (unknown / not simulating).
fn bit(v: &PartView, p: usize, i: usize) -> Option<bool> {
    let val = v.port_vals.get(p).copied().flatten()?;
    match val.bits().get(i) {
        Some(b) if *b == Value::TRUE => Some(true),
        Some(b) if *b == Value::FALSE => Some(false),
        _ => None,
    }
}

fn label(g: &Gfx, v: &PartView, at: (f32, f32), anchor: Align2) {
    let l = v.string("label");
    if !l.is_empty() {
        g.text(at, anchor, &l, 9.0, g.theme.text);
    }
}

fn lit_color(g: &Gfx, on: Option<bool>) -> Color32 {
    match on {
        Some(true) => LIT,
        Some(false) => g.theme.grid_major,
        None => g.theme.fill,
    }
}

struct ButtonPainter;
impl PartPainter for ButtonPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let active = v.string("press") != "passive";
        let pressed = bit(v, 0, 0).map(|b| b == active).unwrap_or(false);
        g.rect((-20.0, -10.0), (0.0, 10.0), Some(th.fill), th.stroke);
        let (a, b) = if pressed {
            ((-16.0, -6.0), (-4.0, 6.0))
        } else {
            ((-18.0, -8.0), (-2.0, 8.0))
        };
        let face = if pressed { th.v_false } else { th.dim };
        g.rect(a, b, Some(face), th.stroke);
        label(g, v, (-10.0, -12.0), Align2::CENTER_BOTTOM);
    }
    fn bounds(&self, _: &PartView) -> Rect4 {
        (-20.0, -10.0, 0.0, 10.0)
    }
}

struct DipPainter;
impl DipPainter {
    fn n(v: &PartView) -> usize {
        v.ports.len().max(1)
    }
}
impl PartPainter for DipPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let n = Self::n(v) as f32;
        g.rect(
            (-30.0, 0.0),
            (0.0, 10.0 * n + 10.0),
            Some(th.fill),
            th.stroke,
        );
        for i in 0..Self::n(v) {
            let y = 10.0 * (i as f32 + 1.0);
            let on = bit(v, i, 0).unwrap_or(false);
            g.rect(
                (-24.0, y - 3.5),
                (-6.0, y + 3.5),
                Some(th.grid_major),
                th.dim,
            );
            let (x0, x1) = if on { (-15.0, -6.0) } else { (-24.0, -15.0) };
            g.rect(
                (x0, y - 3.5),
                (x1, y + 3.5),
                Some(if on { th.v_true } else { th.dim }),
                th.stroke,
            );
            g.text(
                (-27.0, y),
                Align2::RIGHT_CENTER,
                &(i + 1).to_string(),
                6.0,
                th.dim,
            );
            g.line((-6.0, y), (0.0, y), v.port_color(th, i));
        }
        g.text((-15.0, 3.0), Align2::CENTER_CENTER, "ON", 6.0, th.dim);
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        (-34.0, 0.0, 0.0, 10.0 * Self::n(v) as f32 + 10.0)
    }
}

struct JoystickPainter;
impl PartPainter for JoystickPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        g.rect((-30.0, -10.0), (0.0, 20.0), Some(th.fill), th.stroke);
        let pos = |p: usize| -> f32 {
            v.port_vals
                .get(p)
                .copied()
                .flatten()
                .and_then(|x| x.to_u64().map(|n| (n, x.width().get())))
                .map(|(n, w)| {
                    let max = ((1u64 << w.min(32)) - 1).max(1) as f32;
                    (n as f32 / max - 0.5) * 18.0
                })
                .unwrap_or(0.0)
        };
        let c = (-15.0 + pos(0), 5.0 + pos(1));
        g.line((-15.0, 5.0), c, th.dim);
        g.circle(c, 5.5, Some(th.v_x), th.stroke);
        g.circle((-15.0, 5.0), 13.0, None, th.dim);
    }
    fn bounds(&self, _: &PartView) -> Rect4 {
        (-30.0, -10.0, 0.0, 20.0)
    }
}

struct TerminalPainter {
    keyboard: bool,
}
impl TerminalPainter {
    fn rect(&self, v: &PartView) -> Rect4 {
        if self.keyboard {
            (0.0, -5.0, 150.0, 25.0)
        } else {
            let cols = v.int("cols", 32).clamp(1, 120) as f32;
            let rows = v.int("rows", 8).clamp(1, 48) as f32;
            (0.0, -20.0, (cols * 4.0 + 10.0).max(60.0), 25.0 + rows * 7.0)
        }
    }
}
impl PartPainter for TerminalPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let r = self.rect(v);
        g.poly(&rect_poly(r), true, Some(th.fill), th.stroke);
        let name: &[(usize, &str)] = if self.keyboard {
            &[(0, "clr"), (1, "clk"), (2, "re"), (3, "avail"), (4, "char")]
        } else {
            &[(0, "clr"), (1, "clk"), (2, "we"), (3, "data")]
        };
        for &(i, s) in name {
            if let Some(p) = v.ports.get(i) {
                let (x, y) = (p.offset.x as f32, p.offset.y as f32);
                let (at, anchor) = if x >= r.2 - 1.0 {
                    ((x - 3.0, y), Align2::RIGHT_CENTER)
                } else if x <= r.0 + 1.0 {
                    ((x + 4.0, y), Align2::LEFT_CENTER)
                } else if self.keyboard && i == 3 {
                    ((x, y - 4.0), Align2::CENTER_BOTTOM)
                } else {
                    ((x, y + 3.0), Align2::CENTER_TOP)
                };
                g.text(at, anchor, s, 6.0, th.dim);
            }
        }
        let title = if self.keyboard { "Keyboard" } else { "TTY" };
        g.text(
            ((r.0 + r.2) / 2.0, r.3 - 8.0),
            Align2::CENTER_CENTER,
            title,
            10.0,
            th.text,
        );
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union(self.rect(v), port_bounds(v, 0.0))
    }
}

struct LedPainter;
impl PartPainter for LedPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let active_high = v.def.options.boolean("active_high").ok().flatten() != Some(false);
        let on = bit(v, 0, 0).map(|b| b == active_high);
        g.circle((-10.0, 0.0), 8.0, Some(lit_color(g, on)), th.stroke);
        label(g, v, (-10.0, -10.0), Align2::CENTER_BOTTOM);
    }
    fn bounds(&self, _: &PartView) -> Rect4 {
        (-20.0, -10.0, 0.0, 10.0)
    }
}

struct RgbLedPainter;
impl PartPainter for RgbLedPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let ch = |p: usize| -> u8 {
            if bit(v, p, 0) == Some(true) {
                255
            } else {
                30
            }
        };
        let c = Color32::from_rgb(ch(0), ch(1), ch(2));
        g.circle((-10.0, 0.0), 8.0, Some(c), th.stroke);
        for (i, p) in v.ports.iter().enumerate().skip(1) {
            let (x, y) = (p.offset.x as f32, p.offset.y as f32);
            g.line((x, y), (x, y * 0.8), v.port_color(th, i));
        }
        label(g, v, (-10.0, 12.0), Align2::CENTER_TOP);
    }
    fn bounds(&self, _: &PartView) -> Rect4 {
        (-20.0, -10.0, 0.0, 10.0)
    }
}

struct BarPainter;
impl BarPainter {
    fn geom(v: &PartView) -> (usize, f32) {
        let n = v.int("segments", 8).clamp(1, 32) as usize;
        let h = 10.0 * v.int("height", 3).clamp(1, 8) as f32;
        (n, h)
    }
}
impl PartPainter for BarPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let (n, h) = Self::geom(v);
        let one_wire = v.ports.len() == 1;
        g.rect(
            (-5.0, -h),
            (10.0 * n as f32 - 5.0, 0.0),
            Some(th.fill),
            th.stroke,
        );
        for i in 0..n {
            let on = if one_wire { bit(v, 0, i) } else { bit(v, i, 0) };
            let x = 10.0 * i as f32;
            g.rect(
                (x - 3.5, -h + 3.0),
                (x + 3.5, -3.0),
                Some(lit_color(g, on)),
                th.dim,
            );
        }
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        let (n, h) = Self::geom(v);
        union((-5.0, -h, 10.0 * n as f32 - 5.0, 0.0), port_bounds(v, 0.0))
    }
}

struct MatrixPainter;
struct MGrid {
    cols: usize,
    rows: usize,
    mode: &'static str,
}
impl MGrid {
    fn of(v: &PartView) -> MGrid {
        let mode = match v.string("input").as_str() {
            "row" => "row",
            "select" => "select",
            _ => "column",
        };
        MGrid {
            cols: v.int("cols", 5).clamp(1, 32) as usize,
            rows: v.int("rows", 7).clamp(1, 32) as usize,
            mode,
        }
    }
    /// Centre of cell (column `c`, row `r`).
    fn centre(&self, c: usize, r: usize) -> (f32, f32) {
        if self.mode == "column" {
            (
                10.0 * c as f32,
                -10.0 * self.rows as f32 + 5.0 + 10.0 * r as f32,
            )
        } else {
            (5.0 + 10.0 * c as f32, 10.0 * r as f32)
        }
    }
    fn rect(&self) -> Rect4 {
        if self.mode == "column" {
            (
                -5.0,
                -10.0 * self.rows as f32,
                10.0 * self.cols as f32 - 5.0,
                0.0,
            )
        } else {
            (
                0.0,
                -5.0,
                10.0 * self.cols as f32,
                10.0 * self.rows as f32 - 5.0,
            )
        }
    }
}
impl PartPainter for MatrixPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let m = MGrid::of(v);
        g.poly(&rect_poly(m.rect()), true, Some(th.fill), th.stroke);
        for c in 0..m.cols {
            for r in 0..m.rows {
                let on = match m.mode {
                    "column" => bit(v, c, r),
                    "row" => bit(v, r, c),
                    _ => match (bit(v, 0, c), bit(v, 1, r)) {
                        (Some(a), Some(b)) => Some(a && b),
                        _ => None,
                    },
                };
                g.circle(m.centre(c, r), 3.6, Some(lit_color(g, on)), th.dim);
            }
        }
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union(MGrid::of(v).rect(), port_bounds(v, 0.0))
    }
}

struct SegmentPainter {
    hex: bool,
}

/// Segment masks (A=1 … G=64) for hex digits 0..=F.
const HEX: [u8; 16] = [
    0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F, 0x77, 0x7C, 0x39, 0x5E, 0x79, 0x71,
];

impl SegmentPainter {
    /// Lit state of segment `s` (0..=6 = A..G, 7 = DP).
    fn lit(&self, v: &PartView, s: usize) -> Option<bool> {
        if self.hex {
            if s == 7 {
                return if v.ports.len() > 1 {
                    bit(v, 1, 0)
                } else {
                    None
                };
            }
            let n = v.port_vals.first().copied().flatten()?;
            return Some(match n.to_u64() {
                Some(d) => HEX[(d & 15) as usize] >> s & 1 == 1,
                None => s == 6, // unknown: show a dash
            });
        }
        let active_high = v.def.options.boolean("active_high").ok().flatten() != Some(false);
        let raw = if v.string("input") == "bus" {
            bit(v, 0, s)
        } else {
            bit(v, s, 0)
        };
        raw.map(|b| b == active_high)
    }
}

impl PartPainter for SegmentPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        g.rect((0.0, 0.0), (40.0, 60.0), Some(th.fill), th.stroke);
        let hz = |x0: f32, x1: f32, y: f32| {
            vec![
                (x0, y),
                (x0 + 3.0, y - 3.0),
                (x1 - 3.0, y - 3.0),
                (x1, y),
                (x1 - 3.0, y + 3.0),
                (x0 + 3.0, y + 3.0),
            ]
        };
        let vt = |x: f32, y0: f32, y1: f32| {
            vec![
                (x, y0),
                (x + 3.0, y0 + 3.0),
                (x + 3.0, y1 - 3.0),
                (x, y1),
                (x - 3.0, y1 - 3.0),
                (x - 3.0, y0 + 3.0),
            ]
        };
        let segs = [
            hz(9.0, 29.0, 8.0),   // A
            vt(32.0, 9.0, 29.0),  // B
            vt(32.0, 31.0, 51.0), // C
            hz(9.0, 29.0, 52.0),  // D
            vt(8.0, 31.0, 51.0),  // E
            vt(8.0, 9.0, 29.0),   // F
            hz(9.0, 29.0, 30.0),  // G
        ];
        for (s, poly) in segs.iter().enumerate() {
            g.poly(poly, true, Some(lit_color(g, self.lit(v, s))), th.dim);
        }
        let dp = self.lit(v, 7);
        let has_dp = if self.hex {
            v.ports.len() > 1
        } else {
            v.boolean("decimal_point")
        };
        if has_dp {
            g.circle((35.0, 53.0), 2.5, Some(lit_color(g, dp)), th.dim);
        }
        if !self.hex && v.string("input") == "bus" {
            if let Some(p) = v.ports.first() {
                let (x, y) = (p.offset.x as f32, p.offset.y as f32);
                g.line((x, y), (0.0, y), v.port_color(th, 0));
            }
        }
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union((0.0, 0.0, 40.0, 60.0), port_bounds(v, 0.0))
    }
}
