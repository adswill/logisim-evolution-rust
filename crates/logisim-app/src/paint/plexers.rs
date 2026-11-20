//! Painters for the Plexers library (trapezoid bodies like Logisim, with port numbering).

use super::block::{bbox, control_label, index_labels, port_at, port_label, stub};
use super::{port_bounds, union, Gfx, PartPainter, PartView};
use eframe::egui::Align2;

pub fn painter(part: &str) -> Option<&'static dyn PartPainter> {
    Some(match part {
        "Multiplexer" => &PlexerPainter(Kind::Mux),
        "Demultiplexer" => &PlexerPainter(Kind::Demux),
        "Decoder" => &PlexerPainter(Kind::Decoder),
        "Priority Encoder" => &PlexerPainter(Kind::PriEnc),
        "Bit Selector" => &PlexerPainter(Kind::BitSel),
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Mux,
    Demux,
    Decoder,
    PriEnc,
    BitSel,
}

struct PlexerPainter(Kind);

/// Which side of the body is slanted.
#[derive(Clone, Copy)]
enum Lean {
    Right,
    Left,
}

struct Body {
    poly: Vec<(f32, f32)>,
    label: &'static str,
}

fn trapezoid(x0: f32, x1: f32, y0: f32, y1: f32, lean: f32, side: Lean) -> Vec<(f32, f32)> {
    match side {
        Lean::Right => vec![(x0, y0), (x1, y0 + lean), (x1, y1 - lean), (x0, y1)],
        Lean::Left => vec![(x0, y0 + lean), (x1, y0), (x1, y1), (x0, y1 - lean)],
    }
}

/// Min / max y over a port index range.
fn y_span(v: &PartView, r: std::ops::Range<usize>) -> (f32, f32) {
    let ys: Vec<f32> = r.filter_map(|i| port_at(v, i)).map(|p| p.1).collect();
    (
        ys.iter().copied().fold(f32::MAX, f32::min),
        ys.iter().copied().fold(f32::MIN, f32::max),
    )
}

fn sel_bits(v: &PartView) -> usize {
    v.int("select_bits", 1).clamp(1, 8) as usize
}

impl PlexerPainter {
    fn body(&self, v: &PartView) -> Body {
        let n = 1usize << sel_bits(v);
        match self.0 {
            Kind::Mux => {
                let (a, b) = y_span(v, 1..n + 1);
                let x0 = port_at(v, 1).map_or(-30.0, |p| p.0);
                let narrow = v.string("size") == "narrow";
                let lean = match (n == 2, narrow) {
                    (true, true) => 7.0,
                    (true, false) => 10.0,
                    (false, true) => 10.0,
                    (false, false) => 20.0,
                };
                Body {
                    poly: trapezoid(x0, 0.0, a - 10.0, b + 10.0, lean, Lean::Right),
                    label: if narrow { "" } else { "MUX" },
                }
            }
            Kind::Demux => {
                let (a, b) = y_span(v, 0..n);
                let x1 = port_at(v, 0).map_or(30.0, |p| p.0);
                let lean = if n == 2 { 10.0 } else { 20.0 };
                Body {
                    poly: trapezoid(0.0, x1, a - 10.0, b + 10.0, lean, Lean::Left),
                    label: "DMX",
                }
            }
            Kind::Decoder => {
                let (a, b) = y_span(v, 0..n);
                let x1 = port_at(v, 0).map_or(10.0, |p| p.0);
                let flip = port_at(v, 0).is_some_and(|p| p.1 > 0.0);
                // lay out for "select at the bottom" and mirror when the outputs are below
                let (lo, hi) = if flip { (-b, -a) } else { (a, b) };
                let (y0, y1) = (lo - 10.0, if n == 2 { hi + 10.0 } else { hi + 20.0 });
                let lean = if n == 2 { 10.0 } else { 20.0 };
                let mut poly = trapezoid(-20.0, x1, y0, y1, lean, Lean::Left);
                if flip {
                    for p in &mut poly {
                        p.1 = -p.1;
                    }
                    poly.reverse();
                }
                Body {
                    poly,
                    label: "Decd",
                }
            }
            Kind::PriEnc => {
                let (a, b) = y_span(v, 1..n + 1);
                let x0 = port_at(v, 1).map_or(-40.0, |p| p.0);
                Body {
                    poly: vec![
                        (x0, a - 10.0),
                        (0.0, a - 10.0),
                        (0.0, b + 10.0),
                        (x0, b + 10.0),
                    ],
                    label: "Pri",
                }
            }
            Kind::BitSel => {
                let x0 = port_at(v, 1).map_or(-30.0, |p| p.0);
                Body {
                    poly: trapezoid(x0, 0.0, -10.0, 10.0, 9.0, Lean::Right),
                    label: "Sel",
                }
            }
        }
    }
}

impl PartPainter for PlexerPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let n = 1usize << sel_bits(v);
        let body = self.body(v);
        let bb = bbox(&body.poly);
        // wire stubs for ports that float off the body (select, enable, …)
        for i in 0..v.ports.len() {
            stub(g, v, i, &body.poly);
        }
        g.poly(&body.poly, true, Some(th.fill), th.stroke);
        let (cx, cy) = ((bb.0 + bb.2) / 2.0, (bb.1 + bb.3) / 2.0);
        if !body.label.is_empty() {
            let size = if bb.2 - bb.0 < 35.0 { 8.0 } else { 10.0 };
            g.text((cx, cy), Align2::CENTER_CENTER, body.label, size, th.text);
        }
        // numbering of the data side and names of the control ports
        match self.0 {
            Kind::Mux => {
                index_labels(g, v, 1..n + 1, bb);
                control_label(g, v, n + 1, bb, "S");
                control_label(g, v, n + 2, bb, "En");
            }
            Kind::Demux => {
                index_labels(g, v, 0..n, bb);
                control_label(g, v, n, bb, "S");
                control_label(g, v, n + 1, bb, "En");
            }
            Kind::Decoder => {
                index_labels(g, v, 0..n, bb);
            }
            Kind::PriEnc => {
                index_labels(g, v, 1..n + 1, bb);
                control_label(g, v, n + 1, bb, "EI");
                control_label(g, v, n + 2, bb, "EO");
                port_label(g, v, n + 3, bb, "GS");
            }
            Kind::BitSel => {}
        }
    }

    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let bb = bbox(&self.body(v).poly);
        union(bb, port_bounds(v, 0.0))
    }
}
