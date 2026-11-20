//! Painters for the Input/Output library. Indicators read their state from the live port values
//! (an LED lights when its input net is high), so they animate while simulating.

use super::block::{port_label, rect_poly, Rect4};
use super::{port_bounds, union, Gfx, PartPainter, PartView};
use eframe::egui::{Align2, Color32};
use logisim_core::Value;

pub fn painter(part: &str) -> Option<&'static dyn PartPainter> {
    Some(match part {
        "Switch" => &SwitchPainter,
        "Two-Way Switch" => &TwoWayPainter,
        "Slider" => &SliderPainter,
        "Two-Pin LED" => &TwoPinLedPainter,
        "Buzzer" => &BuzzerPainter,
        "Programmable Generator" => &GeneratorPainter,
        "Matrix Keypad" => &KeypadPainter,
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

fn same_value(v: &PartView, a: usize, b: usize) -> bool {
    match (
        v.port_vals.get(a).copied().flatten(),
        v.port_vals.get(b).copied().flatten(),
    ) {
        (Some(x), Some(y)) => x == y && !x.is_unknown() && !x.is_error(),
        _ => false,
    }
}

struct SwitchPainter;
impl PartPainter for SwitchPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        // the switch is closed when both sides carry the same defined value
        let closed = same_value(v, 0, 1);
        g.line((-20.0, 0.0), (-12.0, 0.0), v.port_color(th, 0));
        g.line((12.0, 0.0), (0.0, 0.0), v.port_color(th, 1));
        g.circle((-12.0, 0.0), 2.0, Some(th.fill), th.stroke);
        g.circle((12.0, 0.0), 2.0, Some(th.fill), th.stroke);
        let tip = if closed { (12.0, 0.0) } else { (9.0, -9.0) };
        g.line(
            (-12.0, 0.0),
            tip,
            if closed { th.v_true } else { th.stroke },
        );
    }
    fn bounds(&self, _: &PartView) -> Rect4 {
        (-20.0, -12.0, 0.0, 6.0)
    }
}

struct TwoWayPainter;
impl PartPainter for TwoWayPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        // find the lone "common" port and the two throws
        let (common, a, b) = (0, 1, 2); // port 0 is the common side in both modes
        let pc = v
            .ports
            .get(common)
            .map(|p| (p.offset.x as f32, p.offset.y as f32))
            .unwrap_or((-20.0, 0.0));
        let dir = if pc.0 <= 0.0 { 1.0 } else { -1.0 };
        let hub = (pc.0 + dir * 8.0, pc.1);
        g.line(pc, hub, v.port_color(th, common));
        g.circle(hub, 2.0, Some(th.fill), th.stroke);
        let mut tips = [(0.0, 0.0); 2];
        for (k, i) in [a, b].into_iter().enumerate() {
            let Some(p) = v.ports.get(i) else { continue };
            let (x, y) = (p.offset.x as f32, p.offset.y as f32);
            g.line((x, y), (x - dir * 8.0, y), v.port_color(th, i));
            g.circle((x - dir * 8.0, y), 2.0, Some(th.fill), th.stroke);
            tips[k] = (x - dir * 8.0, y);
        }
        let sel = if same_value(v, common, b) { 1 } else { 0 };
        g.line(hub, tips[sel], th.stroke);
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union((-20.0, -14.0, 20.0, 14.0), port_bounds(v, 2.0))
    }
}

struct SliderPainter;
impl SliderPainter {
    fn rect(v: &PartView) -> Rect4 {
        if v.string("direction") == "right_to_left" {
            (0.0, -10.0, 60.0, 10.0)
        } else {
            (-60.0, -10.0, 0.0, 10.0)
        }
    }
}
impl PartPainter for SliderPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let r = Self::rect(v);
        g.poly(&rect_poly(r), true, Some(th.fill), th.stroke);
        g.line((r.0 + 6.0, 0.0), (r.2 - 6.0, 0.0), th.dim);
        let frac = v
            .port_vals
            .first()
            .copied()
            .flatten()
            .and_then(|x| x.to_u64().map(|n| (n, x.width().get())))
            .map(|(n, w)| n as f32 / ((1u64 << w.min(32)) - 1).max(1) as f32)
            .unwrap_or(0.5);
        let t = if v.string("direction") == "right_to_left" {
            1.0 - frac
        } else {
            frac
        };
        let x = r.0 + 6.0 + (r.2 - r.0 - 12.0) * t;
        g.rect((x - 3.0, -7.0), (x + 3.0, 7.0), Some(th.v_x), th.stroke);
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union(Self::rect(v), port_bounds(v, 0.0))
    }
}

struct TwoPinLedPainter;
impl PartPainter for TwoPinLedPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let (a, b) = (bit(v, 0, 0), bit(v, 1, 0));
        let bipolar = v.boolean("bipolar");
        // current flows from the right pin (port 0) to the left pin (port 1) when 0 is high
        let fill = match (a, b) {
            (Some(true), Some(false)) => LIT,
            (Some(false), Some(true)) if bipolar => Color32::from_rgb(60, 200, 90),
            (Some(_), Some(_)) => th.grid_major,
            _ => th.fill,
        };
        g.line((10.0, 0.0), (8.0, 0.0), v.port_color(th, 0));
        g.line((-10.0, 0.0), (-8.0, 0.0), v.port_color(th, 1));
        g.circle((0.0, 0.0), 8.0, Some(fill), th.stroke);
        label(g, v, (0.0, -10.0), Align2::CENTER_BOTTOM);
    }
    fn bounds(&self, _: &PartView) -> Rect4 {
        (-10.0, -10.0, 10.0, 10.0)
    }
}

struct BuzzerPainter;
const BUZZER: Rect4 = (-40.0, -20.0, 0.0, 20.0);
impl PartPainter for BuzzerPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        g.poly(&rect_poly(BUZZER), true, Some(th.fill), th.stroke);
        // speaker glyph
        g.poly(
            &[
                (-36.0, -4.0),
                (-30.0, -4.0),
                (-22.0, -10.0),
                (-22.0, 10.0),
                (-30.0, 4.0),
                (-36.0, 4.0),
            ],
            true,
            Some(th.dim),
            th.stroke,
        );
        if bit(v, 1, 0) == Some(true) {
            for r in [4.0f32, 8.0] {
                let arc: Vec<(f32, f32)> = (0..=6)
                    .map(|i| {
                        let a = -0.9 + 1.8 * i as f32 / 6.0;
                        (-20.0 + r * a.cos(), r * a.sin() * 1.3)
                    })
                    .collect();
                g.poly(&arc, false, None, LIT);
            }
        }
        for (i, l) in [(0, "f"), (1, "en"), (2, "vol"), (3, "wave")] {
            port_label(g, v, i, BUZZER, l);
        }
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union(BUZZER, port_bounds(v, 0.0))
    }
}

struct GeneratorPainter;
impl PartPainter for GeneratorPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let r = (-50.0, -10.0, 0.0, 10.0);
        g.poly(&rect_poly(r), true, Some(th.fill), th.stroke);
        g.text(
            (-25.0, 0.0),
            Align2::CENTER_CENTER,
            &format!("gen ×{}", v.int("states", 4)),
            9.0,
            th.text,
        );
    }
    fn bounds(&self, _: &PartView) -> Rect4 {
        (-50.0, -10.0, 0.0, 10.0)
    }
}

struct KeypadPainter;
impl KeypadPainter {
    /// (body, columns, rows)
    fn geom(v: &PartView) -> (Rect4, usize, usize) {
        let max_x = v.ports.iter().map(|p| p.offset.x).max().unwrap_or(30) as f32;
        let max_y = v.ports.iter().map(|p| p.offset.y).max().unwrap_or(30) as f32;
        let min_x = v.ports.iter().map(|p| p.offset.x).min().unwrap_or(-20) as f32;
        let min_y = v.ports.iter().map(|p| p.offset.y).min().unwrap_or(-20) as f32;
        let cols = v
            .ports
            .iter()
            .filter(|p| p.offset.y as f32 == max_y)
            .count()
            .max(1);
        let rows = v
            .ports
            .iter()
            .filter(|p| p.offset.x as f32 == max_x)
            .count()
            .max(1);
        ((min_x - 10.0, min_y - 10.0, max_x, max_y), cols, rows)
    }
}
impl PartPainter for KeypadPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let (r, cols, rows) = Self::geom(v);
        g.poly(&rect_poly(r), true, Some(th.fill), th.stroke);
        let keys: Vec<char> = "123A456B789C*0#D".chars().collect();
        let (cw, ch) = ((r.2 - r.0) / cols as f32, (r.3 - r.1) / rows as f32);
        for row in 0..rows {
            for col in 0..cols {
                let (x0, y0) = (r.0 + cw * col as f32, r.1 + ch * row as f32);
                g.rect(
                    (x0 + 1.5, y0 + 1.5),
                    (x0 + cw - 1.5, y0 + ch - 1.5),
                    Some(th.grid_major),
                    th.dim,
                );
                let k = if cols == 4 && rows == 4 {
                    keys[row * 4 + col].to_string()
                } else {
                    (row * cols + col).to_string()
                };
                g.text(
                    (x0 + cw / 2.0, y0 + ch / 2.0),
                    Align2::CENTER_CENTER,
                    &k,
                    (ch * 0.5).min(9.0),
                    th.text,
                );
            }
        }
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union(Self::geom(v).0, port_bounds(v, 0.0))
    }
}
