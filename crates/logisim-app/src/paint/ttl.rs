//! Painter for the TTL library: every 74xx chip is drawn as a DIP package (body, notch, pins,
//! chip number, Vcc/GND labels). The pin count is recovered from the port layout, which follows
//! `AbstractTtlGate`: pin `p` sits on the bottom row for `p <= n/2`, on the top row otherwise.

use super::{Gfx, PartPainter, PartView};
use eframe::egui::Align2;

const PIN_W: f32 = 10.0;
const PIN_H: f32 = 7.0;
const TOP: f32 = -30.0;

/// One painter serves every TTL chip.
pub fn painter(_part: &str) -> Option<&'static dyn PartPainter> {
    Some(&TtlPainter)
}

struct TtlPainter;

/// Package geometry derived from the ports.
struct Package {
    /// number of pins (even)
    pins: i32,
    /// y of the bottom pin row
    bottom: f32,
}

impl Package {
    fn of(v: &PartView) -> Package {
        let mut max_x = 0.0f32;
        let mut bottom = 30.0f32;
        for p in v.ports {
            max_x = max_x.max(p.offset.x as f32);
            bottom = bottom.max(p.offset.y as f32);
        }
        let mut pins = ((max_x + 10.0) / 10.0).round() as i32;
        pins += pins & 1;
        Package {
            pins: pins.clamp(4, 64),
            bottom,
        }
    }
    fn width(&self) -> f32 {
        self.pins as f32 * 10.0
    }
    /// Local centre of datasheet pin `p` (1-based).
    fn pin_pos(&self, p: i32) -> (f32, f32) {
        let i = p - 1;
        let n = self.pins;
        if i < n / 2 {
            ((i * 20 + 10) as f32, self.bottom)
        } else {
            ((n * 10 - (i - n / 2) * 20 - 10) as f32, TOP)
        }
    }
    fn has_port(&self, v: &PartView, p: i32) -> Option<usize> {
        let (x, y) = self.pin_pos(p);
        v.ports
            .iter()
            .position(|q| q.offset.x as f32 == x && q.offset.y as f32 == y)
    }
}

impl PartPainter for TtlPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let pk = Package::of(v);
        let w = pk.width();
        let (y0, y1) = (TOP + PIN_H, pk.bottom - PIN_H);
        let mid = (y0 + y1) / 2.0;

        // pins, coloured with the live value of their port
        for p in 1..=pk.pins {
            let (x, y) = pk.pin_pos(p);
            let (ya, yb) = if y == TOP { (TOP, y0) } else { (y1, pk.bottom) };
            let c = pk.has_port(v, p).map_or(th.idle, |i| v.port_color(th, i));
            g.rect(
                (x - PIN_W / 2.0, ya),
                (x + PIN_W / 2.0, yb),
                Some(c),
                th.stroke,
            );
        }

        // body with the orientation notch on the left edge
        let mut body = vec![(0.0, y0), (w, y0), (w, y1), (0.0, y1), (0.0, mid + 7.0)];
        for i in 0..=10 {
            let t = std::f32::consts::PI * i as f32 / 10.0;
            body.push((7.0 * t.sin(), mid + 7.0 * t.cos()));
        }
        g.poly(&body, true, Some(th.fill), th.stroke);

        // chip number
        g.text(
            (w / 2.0, mid - 2.0),
            Align2::CENTER_CENTER,
            v.name,
            14.0,
            th.text,
        );

        // datasheet pin numbers (inside the body) and power labels
        let n = pk.pins;
        for p in 1..=n {
            let (x, y) = pk.pin_pos(p);
            let (ty, anchor) = if y == TOP {
                (y0 + 3.0, Align2::CENTER_TOP)
            } else {
                (y1 - 3.0, Align2::CENTER_BOTTOM)
            };
            let label = if p == n && pk.has_port(v, p).is_none() {
                "Vcc".to_string()
            } else if p == n / 2 && pk.has_port(v, p).is_none() {
                "GND".to_string()
            } else {
                p.to_string()
            };
            let dim = label.parse::<i32>().is_ok();
            g.text(
                (x, ty),
                anchor,
                &label,
                6.0,
                if dim { th.dim } else { th.text },
            );
        }
    }

    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let pk = Package::of(v);
        (0.0, TOP, pk.width(), pk.bottom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_positions_match_core_layout() {
        let pk = Package {
            pins: 14,
            bottom: 30.0,
        };
        assert_eq!(pk.pin_pos(1), (10.0, 30.0));
        assert_eq!(pk.pin_pos(7), (130.0, 30.0));
        assert_eq!(pk.pin_pos(8), (130.0, -30.0));
        assert_eq!(pk.pin_pos(14), (10.0, -30.0));
        assert_eq!(pk.width(), 140.0);
    }
}
