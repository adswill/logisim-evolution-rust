//! Painters for the Gates library. This is the reference implementation: copy its structure.

use super::{port_bounds, union, Gfx, PartPainter, PartView};

/// Painter for a part of this library, or `None` to fall back to the generic box.
pub fn painter(part: &str) -> Option<&'static dyn PartPainter> {
    Some(match part {
        "AND Gate" => &GatePainter {
            shape: GateShape::And,
            neg: false,
        },
        "NAND Gate" => &GatePainter {
            shape: GateShape::And,
            neg: true,
        },
        "OR Gate" => &GatePainter {
            shape: GateShape::Or,
            neg: false,
        },
        "NOR Gate" => &GatePainter {
            shape: GateShape::Or,
            neg: true,
        },
        "XOR Gate" => &GatePainter {
            shape: GateShape::Xor,
            neg: false,
        },
        "XNOR Gate" => &GatePainter {
            shape: GateShape::Xor,
            neg: true,
        },
        "NOT Gate" => &BufferPainter {
            inverting: true,
            controlled: false,
        },
        "Buffer" => &BufferPainter {
            inverting: false,
            controlled: false,
        },
        "Controlled Buffer" => &BufferPainter {
            inverting: false,
            controlled: true,
        },
        "Controlled Inverter" => &BufferPainter {
            inverting: true,
            controlled: true,
        },
        "Odd Parity" => &LabelBox { title: "2k+1" },
        "Even Parity" => &LabelBox { title: "2k" },
        "PLA" => &LabelBox { title: "PLA" },
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GateShape {
    And,
    Or,
    Xor,
}

struct GatePainter {
    shape: GateShape,
    neg: bool,
}

fn quad(p0: (f32, f32), c: (f32, f32), p1: (f32, f32), n: usize) -> Vec<(f32, f32)> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            (
                u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p1.0,
                u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p1.1,
            )
        })
        .collect()
}

impl GatePainter {
    fn dims(&self, v: &PartView) -> (f32, f32, f32) {
        let size = v.int("size", 50).clamp(30, 70) as f32;
        let tip = if self.neg { -10.0 } else { 0.0 };
        (tip - size, tip, size / 2.0) // back, tip, half height
    }
}

impl PartPainter for GatePainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let (back, tip, h) = self.dims(v);
        let th = g.theme;
        let mut body: Vec<(f32, f32)> = vec![];
        match self.shape {
            GateShape::And => {
                body.push((back, -h));
                body.push((tip - h, -h));
                for i in 0..=16 {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0;
                    body.push((tip - h + h * a.cos(), h * a.sin()));
                }
                body.push((back, h));
            }
            GateShape::Or | GateShape::Xor => {
                let w = tip - back;
                body.extend(quad((back, -h), (back + w * 0.55, -h), (tip, 0.0), 14));
                let mut bot = quad((back, h), (back + w * 0.55, h), (tip, 0.0), 14);
                bot.reverse();
                body.extend(bot);
                body.extend(quad((back, h), (back + 14.0, 0.0), (back, -h), 10));
            }
        }
        let fill = if self.shape == GateShape::And {
            Some(th.fill)
        } else {
            None
        };
        g.poly(&body, true, fill, th.stroke);
        if self.shape == GateShape::Xor {
            g.poly(
                &quad((back - 8.0, h), (back + 6.0, 0.0), (back - 8.0, -h), 10),
                false,
                None,
                th.stroke,
            );
        }
        let back_x = match self.shape {
            GateShape::And => back,
            GateShape::Or => back + 6.0,
            GateShape::Xor => back - 2.0,
        };
        let negated = v.int("negated", 0) as u64;
        for (i, p) in v.ports.iter().enumerate().skip(1) {
            let (px, py) = (p.offset.x as f32, p.offset.y as f32);
            let c = v.port_color(th, i);
            if negated >> (i - 1) & 1 == 1 {
                g.bubble((px + 5.0, py));
                g.line((px + 10.0, py), (back_x, py), c);
            } else {
                g.line((px, py), (back_x, py), c);
            }
        }
        if self.neg {
            g.bubble((-5.0, 0.0));
        }
        let c = v.port_color(th, 0);
        g.line((0.0, 0.0), (if self.neg { -9.5 } else { 0.0 }, 0.0), c);
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let (back, tip, h) = self.dims(v);
        union(
            (back - 10.0, -h - 2.0, tip.max(0.0) + 2.0, h + 2.0),
            port_bounds(v, 4.0),
        )
    }
}

struct BufferPainter {
    inverting: bool,
    controlled: bool,
}

impl PartPainter for BufferPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let tip = if self.inverting { -10.0 } else { 0.0 };
        let base = tip - 15.0;
        if self.inverting {
            g.line((-30.0, 0.0), (base, 0.0), v.port_color(th, 1));
        } else {
            g.line((base - 5.0, 0.0), (base, 0.0), v.port_color(th, 1));
        }
        g.poly(
            &[(base, -9.0), (base, 9.0), (tip, 0.0)],
            true,
            Some(th.fill),
            th.stroke,
        );
        if self.inverting {
            g.bubble((-5.0, 0.0));
        } else {
            g.line((tip, 0.0), (0.0, 0.0), v.port_color(th, 0));
        }
        if self.controlled {
            g.line((-10.0, -10.0), (-10.0, -4.0), v.port_color(th, 2));
        }
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        union((-32.0, -12.0, 2.0, 12.0), port_bounds(v, 4.0))
    }
}

/// Plain labelled rectangle spanning the ports (parity gates, PLA).
struct LabelBox {
    title: &'static str,
}

impl LabelBox {
    fn rect(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let b = port_bounds(v, 0.0);
        if self.title == "PLA" {
            (b.0, b.1 - 10.0, b.2, b.3 + 30.0)
        } else {
            (b.0, b.1 - 10.0, 0.0, b.3 + 10.0)
        }
    }
}

impl PartPainter for LabelBox {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let r = self.rect(v);
        g.rect((r.0, r.1), (r.2, r.3), Some(g.theme.fill), g.theme.stroke);
        g.text(
            ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0),
            eframe::egui::Align2::CENTER_CENTER,
            self.title,
            11.0,
            g.theme.text,
        );
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        union(self.rect(v), port_bounds(v, 0.0))
    }
}
