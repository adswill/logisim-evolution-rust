//! Painters for the Wiring library.

use super::{port_bounds, union, value_text, Gfx, PartPainter, PartView};
use eframe::egui::Align2;

pub fn painter(part: &str) -> Option<&'static dyn PartPainter> {
    Some(match part {
        "Pin" => &PinPainter,
        "Clock" => &ClockPainter,
        "Splitter" => &SplitterPainter,
        "Tunnel" => &TunnelPainter,
        "Pull Resistor" => &PullPainter,
        "Constant" => &ConstantPainter,
        "Probe" => &ProbePainter,
        "Power" => &PowerPainter,
        "Ground" => &GroundPainter,
        "Transistor" => &SwitchPainter { gates: 1 },
        "Transmission Gate" => &SwitchPainter { gates: 2 },
        "Bit Extender" => &ExtenderPainter,
        "Do Not Connect" => &DncPainter,
        "Power On Reset" => &PorPainter,
        _ => return None,
    })
}

fn digits(width: i64) -> f32 {
    ((width as f32) / 4.0).ceil().max(1.0)
}

struct PinPainter;
impl PartPainter for PinPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let width = v.int("width", 1);
        let output = v.boolean("output");
        let w = if width <= 1 {
            20.0
        } else {
            10.0 * digits(width) + 8.0
        };
        let live = v.port_vals.first().copied().flatten();
        let (txt, color) = if output {
            (
                live.map_or("?".to_string(), |x| value_text(&x)),
                th.value_color(live),
            )
        } else {
            let val = v.int("value", 0) as u64;
            let n = logisim_core::Value::known(
                logisim_core::BitWidth::new(width.clamp(1, 64) as u8),
                val,
            );
            (value_text(&n), th.value_color(Some(n)))
        };
        let cx = if output { w / 2.0 } else { -w / 2.0 };
        if output {
            g.circle((cx, 0.0), w / 2.0, Some(th.fill), th.stroke);
        } else {
            g.rect((-w, -10.0), (0.0, 10.0), Some(th.fill), th.stroke);
        }
        g.text((cx, 0.0), Align2::CENTER_CENTER, &txt, 11.0, color);
        let label = v.string("label");
        if !label.is_empty() {
            g.text((cx, -12.0), Align2::CENTER_BOTTOM, &label, 9.0, th.dim);
        }
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let width = v.int("width", 1);
        let w = if width <= 1 {
            20.0
        } else {
            10.0 * digits(width) + 8.0
        };
        if v.boolean("output") {
            (-4.0, -12.0, w + 2.0, 12.0)
        } else {
            (-w - 2.0, -12.0, 4.0, 12.0)
        }
    }
}

struct ClockPainter;
impl PartPainter for ClockPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let c = v.port_color(th, 0);
        g.rect((-20.0, -10.0), (0.0, 10.0), Some(th.fill), c);
        g.poly(
            &[
                (-17.0, 4.0),
                (-12.0, 4.0),
                (-12.0, -4.0),
                (-7.0, -4.0),
                (-7.0, 4.0),
                (-3.0, 4.0),
            ],
            false,
            None,
            c,
        );
    }
    fn bounds(&self, _: &PartView) -> (f32, f32, f32, f32) {
        (-22.0, -12.0, 4.0, 12.0)
    }
}

struct SplitterPainter;
impl PartPainter for SplitterPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let ends = &v.ports[1.min(v.ports.len())..];
        if ends.is_empty() {
            return;
        }
        let ys: Vec<f32> = ends.iter().map(|p| p.offset.y as f32).collect();
        let (lo, hi) = (
            ys.iter().cloned().fold(f32::MAX, f32::min),
            ys.iter().cloned().fold(f32::MIN, f32::max),
        );
        let spine = 6.0;
        let bus_c = v.port_color(th, 0);
        g.line((0.0, 0.0), (spine, 0.0), bus_c);
        g.line((spine, lo.min(0.0)), (spine, hi.max(0.0)), th.stroke);
        let bit_end: Vec<u8> = v
            .string("bit_end")
            .split(',')
            .filter_map(|t| t.trim().parse().ok())
            .collect();
        for (i, p) in ends.iter().enumerate() {
            let (px, py) = (p.offset.x as f32, p.offset.y as f32);
            g.line((spine, py), (px, py), v.port_color(th, i + 1));
            let bits: Vec<usize> = bit_end
                .iter()
                .enumerate()
                .filter(|(_, e)| **e as usize == i + 1)
                .map(|(b, _)| b)
                .collect();
            let label = match (bits.first(), bits.last()) {
                (Some(a), Some(b)) if a == b => format!("{a}"),
                (Some(a), Some(b)) => format!("{a}-{b}"),
                _ => String::new(),
            };
            g.text(
                (px + 3.0, py - 1.0),
                Align2::LEFT_BOTTOM,
                &label,
                8.0,
                th.dim,
            );
        }
        g.text(
            (-3.0, -2.0),
            Align2::RIGHT_BOTTOM,
            &v.int("width", 1).to_string(),
            8.0,
            th.dim,
        );
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        port_bounds(v, 6.0)
    }
}

struct TunnelPainter;
impl PartPainter for TunnelPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let name = v.string("name");
        let w = 6.5 * name.chars().count().max(1) as f32 + 6.0;
        let c = v.port_color(th, 0);
        g.poly(
            &[
                (0.0, 0.0),
                (-9.0, -8.0),
                (-9.0 - w, -8.0),
                (-9.0 - w, 8.0),
                (-9.0, 8.0),
            ],
            true,
            Some(th.fill),
            c,
        );
        g.text(
            (-9.0 - w / 2.0, 0.0),
            Align2::CENTER_CENTER,
            &name,
            10.0,
            th.text,
        );
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let w = 6.5 * v.string("name").chars().count().max(1) as f32 + 6.0;
        (-12.0 - w, -10.0, 3.0, 10.0)
    }
}

struct PullPainter;
impl PartPainter for PullPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let c = v.port_color(th, 0);
        let zig = [
            (0.0, 0.0),
            (0.0, 3.0),
            (-4.0, 6.0),
            (4.0, 10.0),
            (-4.0, 14.0),
            (4.0, 18.0),
            (0.0, 21.0),
            (0.0, 24.0),
        ];
        g.poly(&zig, false, None, th.stroke);
        let sym = match v.string("pull").as_str() {
            "up" => "1",
            "error" => "E",
            _ => "0",
        };
        g.text((6.0, 22.0), Align2::LEFT_CENTER, sym, 9.0, c);
    }
    fn bounds(&self, _: &PartView) -> (f32, f32, f32, f32) {
        (-8.0, -3.0, 14.0, 28.0)
    }
}

struct ConstantPainter;
impl PartPainter for ConstantPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let width = v.int("width", 1);
        let val = v.int("value", 0) as u64;
        let n =
            logisim_core::Value::known(logisim_core::BitWidth::new(width.clamp(1, 64) as u8), val);
        let txt = value_text(&n);
        let w = 7.0 * txt.chars().count().max(1) as f32 + 6.0;
        g.line((-1.0, 0.0), (0.0, 0.0), th.stroke);
        g.rect((-w, -7.0), (-1.0, 7.0), None, th.dim);
        g.text(
            (-w / 2.0 - 0.5, 0.0),
            Align2::CENTER_CENTER,
            &txt,
            10.0,
            th.value_color(Some(n)),
        );
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let width = v.int("width", 1);
        let n = logisim_core::Value::known(
            logisim_core::BitWidth::new(width.clamp(1, 64) as u8),
            v.int("value", 0) as u64,
        );
        let w = 7.0 * value_text(&n).chars().count().max(1) as f32 + 6.0;
        (-w - 2.0, -9.0, 3.0, 9.0)
    }
}

struct ProbePainter;
impl PartPainter for ProbePainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let live = v.port_vals.first().copied().flatten();
        let txt = live.map_or("?".into(), |x| value_text(&x));
        let w = 8.0 * txt.chars().count().max(1) as f32 + 8.0;
        g.rect((0.0, -10.0), (w, 10.0), Some(th.fill), th.dim);
        g.text(
            (w / 2.0, 0.0),
            Align2::CENTER_CENTER,
            &txt,
            11.0,
            th.value_color(live),
        );
    }
    fn bounds(&self, _: &PartView) -> (f32, f32, f32, f32) {
        (-2.0, -12.0, 40.0, 12.0)
    }
}

struct PowerPainter;
impl PartPainter for PowerPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        g.line((0.0, 0.0), (0.0, -5.0), v.port_color(th, 0));
        g.poly(
            &[(-7.0, -5.0), (7.0, -5.0), (0.0, -15.0)],
            true,
            Some(th.v_true),
            th.stroke,
        );
    }
    fn bounds(&self, _: &PartView) -> (f32, f32, f32, f32) {
        (-9.0, -17.0, 9.0, 3.0)
    }
}

struct GroundPainter;
impl PartPainter for GroundPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        g.line((0.0, 0.0), (0.0, 6.0), v.port_color(th, 0));
        g.line((-8.0, 6.0), (8.0, 6.0), th.stroke);
        g.line((-5.0, 9.0), (5.0, 9.0), th.stroke);
        g.line((-2.0, 12.0), (2.0, 12.0), th.stroke);
    }
    fn bounds(&self, _: &PartView) -> (f32, f32, f32, f32) {
        (-10.0, -3.0, 10.0, 14.0)
    }
}

/// Transistor (1 gate) and transmission gate (2 gates).
struct SwitchPainter {
    gates: usize,
}
impl PartPainter for SwitchPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        g.line((-40.0, 0.0), (-32.0, 0.0), v.port_color(th, 1));
        g.line((-8.0, 0.0), (0.0, 0.0), v.port_color(th, 0));
        g.rect((-32.0, -8.0), (-8.0, 8.0), Some(th.fill), th.stroke);
        let label = if self.gates == 1 {
            v.string("type").to_uppercase()
        } else {
            "TG".into()
        };
        g.text((-20.0, 0.0), Align2::CENTER_CENTER, &label, 10.0, th.text);
        for i in 0..self.gates {
            let p = &v.ports[2 + i];
            let py = p.offset.y as f32;
            let edge: f32 = if p.offset.y < 0 { -8.0 } else { 8.0 };
            let p_type = if self.gates == 1 {
                v.string("type") == "p"
            } else {
                i == 0
            };
            let (tail, bubble_y) = if p_type {
                (edge + edge.signum() * 9.0, edge + edge.signum() * 4.5)
            } else {
                (edge, 0.0)
            };
            if p_type {
                g.bubble((-20.0, bubble_y));
            }
            g.line((-20.0, py), (-20.0, tail), v.port_color(th, 2 + i));
        }
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        port_bounds(v, 6.0)
    }
}

struct ExtenderPainter;
impl PartPainter for ExtenderPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        g.line((-40.0, 0.0), (-34.0, 0.0), v.port_color(th, 1));
        g.line((-6.0, 0.0), (0.0, 0.0), v.port_color(th, 0));
        g.rect((-34.0, -12.0), (-6.0, 12.0), Some(th.fill), th.stroke);
        let t = format!("{}→{}", v.int("in_width", 8), v.int("out_width", 16));
        g.text((-20.0, 0.0), Align2::CENTER_CENTER, &t, 9.0, th.text);
        if v.ports.len() > 2 {
            g.line((-20.0, -20.0), (-20.0, -12.0), v.port_color(th, 2));
        }
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        union((-42.0, -14.0, 2.0, 14.0), port_bounds(v, 4.0))
    }
}

struct DncPainter;
impl PartPainter for DncPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let c = g.theme.v_err;
        let _ = v;
        g.line((-5.0, -5.0), (5.0, 5.0), c);
        g.line((-5.0, 5.0), (5.0, -5.0), c);
    }
    fn bounds(&self, _: &PartView) -> (f32, f32, f32, f32) {
        (-7.0, -7.0, 7.0, 7.0)
    }
}

struct PorPainter;
impl PartPainter for PorPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        g.rect((-30.0, -10.0), (0.0, 10.0), Some(th.fill), th.stroke);
        let arrow = if v.string("transition") == "low_to_high" {
            "↑"
        } else {
            "↓"
        };
        g.text(
            (-15.0, 0.0),
            Align2::CENTER_CENTER,
            &format!("POR {arrow}"),
            9.0,
            th.text,
        );
    }
    fn bounds(&self, _: &PartView) -> (f32, f32, f32, f32) {
        (-30.0, -10.0, 0.0, 10.0)
    }
}
