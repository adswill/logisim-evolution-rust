//! Painters for the Arithmetic library: a box with the operation symbol and labelled ports.
//! Port order follows the component docs (port 0 is the primary output on the right edge).

use super::block::{port_at, port_label, rect_poly};
use super::{port_bounds, union, Gfx, PartPainter, PartView};
use eframe::egui::Align2;

const NONE: &str = "";

struct BlockPainter {
    symbol: &'static str,
    /// Label per port index (`""` = none).
    labels: &'static [&'static str],
    fp: bool,
}

macro_rules! b {
    ($sym:expr, [$($l:expr),*]) => {
        &BlockPainter { symbol: $sym, labels: &[$($l),*], fp: false }
    };
}
macro_rules! fp {
    ($sym:expr, [$($l:expr),*]) => {
        &BlockPainter { symbol: $sym, labels: &[$($l),*], fp: true }
    };
}

pub fn painter(part: &str) -> Option<&'static dyn PartPainter> {
    Some(match part {
        "Adder" => b!("+", [NONE, "c out", "A", "B", "c in"]),
        "Subtractor" => b!("−", [NONE, "b out", "A", "B", "b in"]),
        "Multiplier" => b!("×", [NONE, "high", "A", "B", "c in"]),
        "Divider" => b!("÷", [NONE, "rem", "A", "B", "upper"]),
        "Negator" => b!("−x", [NONE, NONE]),
        "Exponentiator" => b!("x^y", [NONE, "high", "x", "y"]),
        "Square Root" => b!("√", [NONE, "rem", "in", "upper"]),
        "Absolute Value" => b!("|x|", [NONE, "ovf", NONE]),
        "Comparator" => b!("cmp", ["=", ">", "<", "A", "B"]),
        "Min/Max" => b!("min\nmax", ["min", "max", "A", "B"]),
        "Shifter" => b!("shift", [NONE, "in", "by"]),
        "Bit Adder" => b!("Σ1", [NONE]),
        "Bit Finder" => b!("find", ["index", "found", "in"]),
        "FP Adder" => fp!("FP +", [NONE, "err", "A", "B"]),
        "FP Subtractor" => fp!("FP −", [NONE, "err", "A", "B"]),
        "FP Multiplier" => fp!("FP ×", [NONE, "err", "A", "B"]),
        "FP Divider" => fp!("FP ÷", [NONE, "err", "A", "B"]),
        "FP Negator" => fp!("FP −x", [NONE, "err", NONE]),
        "FP Exponentiator" => fp!("FP x^y", [NONE, "err", "x", "y"]),
        "FP Logarithm" => fp!("FP log", [NONE, "err", "x", "base"]),
        "FP Square Root" => fp!("FP √", [NONE, "err", NONE]),
        "FP Absolute Value" => fp!("FP |x|", [NONE, "err", NONE]),
        "FP Comparator" => fp!("FP cmp", ["=", ">", "<", "err", "A", "B"]),
        "FP Min/Max" => fp!("FP\nmin/max", [NONE, "err", "A", "B"]),
        "FP Round" => fp!("FP round", [NONE, "err", NONE]),
        "FP Trigonometry" => fp!("FP trig", [NONE, NONE, NONE, "err", NONE]),
        "FP Classifier" => fp!(
            "FP\nclass",
            ["norm", "neg", "zero", "sub", "inf", "NaN", NONE]
        ),
        "FP to FP" => fp!("FP→FP", [NONE, "err", NONE]),
        "FP to Int" => fp!("FP→int", [NONE, "err", NONE]),
        "Int to FP" => fp!("int→FP", [NONE, "err", NONE]),
        "FP Constant" => &ConstPainter,
        _ => return None,
    })
}

impl BlockPainter {
    fn rect(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let mut x0 = -40.0f32;
        for p in v.ports {
            x0 = x0.min(p.offset.x as f32);
        }
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for p in v.ports {
            if p.offset.x as f32 == x0 {
                lo = lo.min(p.offset.y as f32);
                hi = hi.max(p.offset.y as f32);
            }
        }
        let (mut y0, mut y1) = (-20.0f32, 20.0f32);
        if lo <= hi {
            y0 = y0.min(lo - 10.0);
            y1 = y1.max(hi + 10.0);
        }
        // ports sitting on the right edge beyond the left column
        for p in v.ports {
            y0 = y0.min(p.offset.y as f32 - if p.offset.x == 0 { 10.0 } else { 0.0 });
            y1 = y1.max(p.offset.y as f32 + if p.offset.x == 0 { 10.0 } else { 0.0 });
        }
        (x0, y0, 0.0, y1)
    }
}

fn symbol_for(sym: &'static str, v: &PartView) -> String {
    if sym == "shift" {
        return match v.string("shift").as_str() {
            "logical_left" => "<<",
            "logical_right" => ">>",
            "arithmetic_right" => ">>>",
            "rotate_left" => "rol",
            "rotate_right" => "ror",
            _ => "shift",
        }
        .to_string();
    }
    sym.to_string()
}

impl PartPainter for BlockPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let r = self.rect(v);
        g.poly(&rect_poly(r), true, Some(th.fill), th.stroke);
        let (cx, cy) = ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0);
        let sym = symbol_for(self.symbol, v);
        let size = if sym.chars().count() > 3 { 9.0 } else { 14.0 };
        g.text((cx, cy), Align2::CENTER_CENTER, &sym, size, th.text);
        for (i, l) in self.labels.iter().enumerate() {
            if !l.is_empty() {
                port_label(g, v, i, r, l);
            }
        }
        // port 0 keeps its stub to the body edge when the body does not reach it
        if let Some(p) = port_at(v, 0) {
            if p.0 > r.2 {
                g.line((r.2, p.1), p, v.port_color(th, 0));
            }
        }
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        union(self.rect(v), port_bounds(v, 0.0))
    }
}

struct ConstPainter;
impl PartPainter for ConstPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let value = v.string("value");
        let w = ((value.chars().count() as f32 * 7.0 + 14.0) / 10.0)
            .ceil()
            .max(4.0)
            * 10.0;
        let r = (-w, -10.0, 0.0, 10.0);
        g.poly(&rect_poly(r), true, Some(th.fill), th.stroke);
        g.text(
            ((r.0 + r.2) / 2.0, 0.0),
            Align2::CENTER_CENTER,
            &value,
            10.0,
            th.text,
        );
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let w = ((v.string("value").chars().count() as f32 * 7.0 + 14.0) / 10.0)
            .ceil()
            .max(4.0)
            * 10.0;
        (-w, -10.0, 0.0, 10.0)
    }
}
