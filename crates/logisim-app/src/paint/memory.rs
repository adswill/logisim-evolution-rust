//! Painters for the Memory library (classic layout): flip-flops, register, counter, shift
//! register, random generator, RAM and ROM. Every control port gets a short label, and clock
//! inputs get the usual triangle.

use super::block::{clock_mark, port_at, port_label, rect_poly, Rect4};
use super::{port_bounds, union, Gfx, PartPainter, PartView};
use eframe::egui::Align2;

pub fn painter(part: &str) -> Option<&'static dyn PartPainter> {
    Some(match part {
        "D Flip-Flop" => &FlipFlopPainter {
            title: "D",
            inputs: &["D"],
        },
        "T Flip-Flop" => &FlipFlopPainter {
            title: "T",
            inputs: &["T"],
        },
        "J-K Flip-Flop" => &FlipFlopPainter {
            title: "J-K",
            inputs: &["J", "K"],
        },
        "S-R Flip-Flop" => &FlipFlopPainter {
            title: "S-R",
            inputs: &["S", "R"],
        },
        "Register" => &Boxed {
            rect: (-30.0, -20.0, 0.0, 20.0),
            title: "Reg",
            // (port index, label); a leading '>' marks a clock
            labels: &[(1, "D"), (2, ">"), (3, "clr"), (4, "en")],
        },
        "Counter" => &Boxed {
            rect: (-30.0, -20.0, 0.0, 20.0),
            title: "Ctr",
            labels: &[
                (1, "D"),
                (2, ">"),
                (3, "clr"),
                (4, "ld"),
                (5, "↕"),
                (6, "en"),
                (7, "ct"),
            ],
        },
        "Random" => &Boxed {
            rect: (-40.0, -20.0, 0.0, 20.0),
            title: "RNG",
            labels: &[(1, ">"), (2, "next"), (3, "rst")],
        },
        "Shift Register" => &ShiftPainter,
        "RAM" => &MemPainter { ram: true },
        "ROM" => &MemPainter { ram: false },
        _ => return None,
    })
}

fn draw_labels(g: &Gfx, v: &PartView, r: Rect4, labels: &[(usize, &str)]) {
    for &(i, l) in labels {
        if l == ">" {
            clock_mark(g, v, i, r);
        } else {
            port_label(g, v, i, r, l);
        }
    }
}

fn frame(g: &Gfx, r: Rect4, title: &str) {
    g.poly(&rect_poly(r), true, Some(g.theme.fill), g.theme.stroke);
    let small = r.2 - r.0 <= 40.0;
    g.text(
        (
            (r.0 + r.2) / 2.0 + if small { 4.0 } else { 0.0 },
            (r.1 + r.3) / 2.0 - if small { 2.0 } else { 0.0 },
        ),
        Align2::CENTER_CENTER,
        title,
        if small || title.chars().count() > 4 {
            9.0
        } else {
            12.0
        },
        g.theme.text,
    );
}

struct Boxed {
    rect: Rect4,
    title: &'static str,
    labels: &'static [(usize, &'static str)],
}

impl PartPainter for Boxed {
    fn paint(&self, g: &Gfx, v: &PartView) {
        frame(g, self.rect, self.title);
        draw_labels(g, v, self.rect, self.labels);
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union(self.rect, port_bounds(v, 0.0))
    }
}

struct FlipFlopPainter {
    title: &'static str,
    inputs: &'static [&'static str],
}

impl PartPainter for FlipFlopPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let r = (-40.0, -10.0, 0.0, 30.0);
        frame(g, r, self.title);
        port_label(g, v, 0, r, "Q");
        port_label(g, v, 1, r, "!Q");
        let k = self.inputs.len();
        for (j, name) in self.inputs.iter().enumerate() {
            port_label(g, v, 2 + j, r, name);
        }
        clock_mark(g, v, 2 + k, r);
        port_label(g, v, 3 + k, r, "clr");
        port_label(g, v, 4 + k, r, "pre");
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union((-40.0, -10.0, 0.0, 30.0), port_bounds(v, 0.0))
    }
}

struct ShiftPainter;
impl ShiftPainter {
    fn rect(v: &PartView) -> Rect4 {
        let x0 = port_at(v, 1).map_or(-30.0, |p| p.0);
        (x0, -20.0, 0.0, 20.0)
    }
}
impl PartPainter for ShiftPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let r = Self::rect(v);
        frame(g, r, "Shift reg");
        draw_labels(
            g,
            v,
            r,
            &[(1, "in"), (2, "en"), (3, ">"), (4, "clr"), (5, "ld")],
        );
        // parallel stages
        let stages = v.ports.len().saturating_sub(6) / 2;
        for s in 0..stages {
            port_label(g, v, 6 + 2 * s, r, &s.to_string());
            port_label(g, v, 7 + 2 * s, r, &s.to_string());
        }
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union(Self::rect(v), port_bounds(v, 0.0))
    }
}

struct MemPainter {
    ram: bool,
}

impl MemPainter {
    fn choice(v: &PartView, key: &str, default: &'static str) -> String {
        let s = v.string(key);
        if s.is_empty() {
            default.to_string()
        } else {
            s
        }
    }
    fn rect(&self, v: &PartView) -> Rect4 {
        let (mut x0, mut y0, mut y1) = (-240.0f32, 0.0f32, 0.0f32);
        for p in v.ports {
            x0 = x0.min(p.offset.x as f32);
            y0 = y0.min(p.offset.y as f32);
            y1 = y1.max(p.offset.y as f32);
        }
        (x0, y0 - 10.0, 0.0, (y1 + 20.0).max(y0 + 70.0))
    }
}

fn words(bits: i64) -> String {
    let n = 1u64 << bits.clamp(0, 40);
    match n {
        n if n >= 1 << 30 => format!("{}G", n >> 30),
        n if n >= 1 << 20 => format!("{}M", n >> 20),
        n if n >= 1 << 10 => format!("{}K", n >> 10),
        n => n.to_string(),
    }
}

impl PartPainter for MemPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let r = self.rect(v);
        let ram = self.ram;
        let addr_bits = v.int("addr_bits", 8);
        let data_bits = v.int("data_bits", 8);
        let line = Self::choice(v, "line", "single");
        let lwords = match line.as_str() {
            "dual" => 2,
            "quad" => 4,
            "octo" => 8,
            _ => 1,
        };
        let line_mode = ram && Self::choice(v, "enables", "byte") == "line";
        let separate = ram && Self::choice(v, "bus", "separate") != "bidir";
        let sync = ram
            && matches!(
                Self::choice(v, "trigger", "rising").as_str(),
                "rising" | "falling"
            );
        let n_out = if !ram || line_mode { lwords } else { 1 };
        let n_in = if separate { n_out } else { 0 };
        let byte_mode = ram && !line_mode;
        let n_oe = usize::from(ram && (!separate || byte_mode));
        let n_we = usize::from(ram);
        let n_clk = usize::from(ram && (sync || line_mode));
        let n_le = if line_mode && lwords > 1 { lwords } else { 0 };
        let n_be = if byte_mode && v.boolean("byte_enables") && sync && data_bits >= 9 {
            ((data_bits + 7) >> 3) as usize
        } else {
            0
        };
        let n_clr = usize::from(ram && v.boolean("clear_pin"));

        g.poly(&rect_poly(r), true, Some(th.fill), th.stroke);
        let title = if ram { "RAM" } else { "ROM" };
        let cx = (r.0 + r.2) / 2.0;
        let cy = (r.1 + r.3) / 2.0;
        g.text(
            (cx, cy),
            Align2::CENTER_CENTER,
            &format!("{title}\n{} × {} bit", words(addr_bits), data_bits),
            13.0,
            th.text,
        );

        let mut i = 0;
        for k in 0..n_out {
            let l = if n_out > 1 {
                format!("D{k}")
            } else {
                "D".to_string()
            };
            port_label(g, v, i, r, if separate || !ram { &l } else { "D" });
            i += 1;
        }
        port_label(g, v, i, r, "A");
        i += 1;
        for k in 0..n_in {
            let l = if n_in > 1 {
                format!("Din{k}")
            } else {
                "Din".to_string()
            };
            port_label(g, v, i, r, &l);
            i += 1;
        }
        if n_oe == 1 {
            port_label(g, v, i, r, "OE");
            i += 1;
        }
        if n_we == 1 {
            port_label(g, v, i, r, "WE");
            i += 1;
        }
        if n_clk == 1 {
            clock_mark(g, v, i, r);
            i += 1;
        }
        for k in 0..n_le {
            port_label(g, v, i, r, &format!("LE{k}"));
            i += 1;
        }
        for k in 0..n_be {
            port_label(g, v, i, r, &format!("BE{}", n_be - k - 1));
            i += 1;
        }
        if n_clr == 1 {
            port_label(g, v, i, r, "clr");
        }
    }
    fn bounds(&self, v: &PartView) -> Rect4 {
        union(self.rect(v), port_bounds(v, 0.0))
    }
}
