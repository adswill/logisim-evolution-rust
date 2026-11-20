//! Shared helpers for library painters: stubs from ports to a body outline, labels placed
//! *inside* a body (so they stay upright and readable when the part is rotated), clock marks.

use super::{rotate_f, Gfx, PartView};
use eframe::egui::{Align2, Color32};

/// Axis-aligned local rectangle `(min x, min y, max x, max y)`.
pub type Rect4 = (f32, f32, f32, f32);

/// Bounding box of a point list.
pub fn bbox(pts: &[(f32, f32)]) -> Rect4 {
    let mut r = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in pts {
        r = (r.0.min(x), r.1.min(y), r.2.max(x), r.3.max(y));
    }
    r
}

pub fn rect_poly(r: Rect4) -> Vec<(f32, f32)> {
    vec![(r.0, r.1), (r.2, r.1), (r.2, r.3), (r.0, r.3)]
}

/// Port `i` as a local point, if it exists.
pub fn port_at(v: &PartView, i: usize) -> Option<(f32, f32)> {
    v.ports
        .get(i)
        .map(|p| (p.offset.x as f32, p.offset.y as f32))
}

/// Where the vertical line through `p` meets the (convex) polygon edge nearest to `p`.
fn vertical_hit(poly: &[(f32, f32)], p: (f32, f32)) -> Option<f32> {
    let mut best: Option<f32> = None;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.0 - b.0).abs() < 1e-3 || p.0 < a.0.min(b.0) - 1e-3 || p.0 > a.0.max(b.0) + 1e-3 {
            continue;
        }
        let y = a.1 + (b.1 - a.1) * (p.0 - a.0) / (b.0 - a.0);
        if best.is_none_or(|o| (y - p.1).abs() < (o - p.1).abs()) {
            best = Some(y);
        }
    }
    best
}

/// Draw the wire stub joining port `i` to the polygon above or below it, in the port's live colour.
pub fn stub(g: &Gfx, v: &PartView, i: usize, poly: &[(f32, f32)]) {
    let Some(p) = port_at(v, i) else { return };
    if let Some(y) = vertical_hit(poly, p) {
        if (y - p.1).abs() > 0.5 {
            g.line(p, (p.0, y), v.port_color(g.theme, i));
        }
    }
}

/// Text at local point `at`, laid out so it extends *into* the part along the local direction
/// `inward` (-1/0/1 per axis) even after the part is rotated. Text itself stays upright.
pub fn inside_text(g: &Gfx, at: (f32, f32), inward: (f32, f32), s: &str, size: f32, c: Color32) {
    let (rx, ry) = rotate_f(g.facing, inward.0, inward.1);
    let h = if rx > 0.5 {
        0
    } else if rx < -0.5 {
        2
    } else {
        1
    };
    let vv = if ry > 0.5 {
        0
    } else if ry < -0.5 {
        2
    } else {
        1
    };
    let anchor = match (h, vv) {
        (0, 0) => Align2::LEFT_TOP,
        (0, 1) => Align2::LEFT_CENTER,
        (0, _) => Align2::LEFT_BOTTOM,
        (1, 0) => Align2::CENTER_TOP,
        (1, 1) => Align2::CENTER_CENTER,
        (1, _) => Align2::CENTER_BOTTOM,
        (_, 0) => Align2::RIGHT_TOP,
        (_, 1) => Align2::RIGHT_CENTER,
        _ => Align2::RIGHT_BOTTOM,
    };
    g.text(
        (at.0 + inward.0 * 5.5, at.1 + inward.1 * 5.5),
        anchor,
        s,
        size,
        c,
    );
}

/// Label for port `i`, placed inside `body` next to the edge the port sits on.
pub fn port_label(g: &Gfx, v: &PartView, i: usize, body: Rect4, s: &str) {
    let Some(p) = port_at(v, i) else { return };
    let inward = if (p.0 - body.0).abs() < 1.0 {
        (1.0, 0.0)
    } else if (p.0 - body.2).abs() < 1.0 {
        (-1.0, 0.0)
    } else if (p.1 - body.1).abs() < 1.0 {
        (0.0, 1.0)
    } else if (p.1 - body.3).abs() < 1.0 {
        (0.0, -1.0)
    } else {
        return;
    };
    inside_text(g, p, inward, s, 7.0, g.theme.dim);
}

/// Small clock triangle on the edge of `body` at port `i`.
pub fn clock_mark(g: &Gfx, v: &PartView, i: usize, body: Rect4) {
    let Some(p) = port_at(v, i) else { return };
    let (dx, dy) = if (p.0 - body.0).abs() < 1.0 {
        (1.0, 0.0)
    } else if (p.0 - body.2).abs() < 1.0 {
        (-1.0, 0.0)
    } else if (p.1 - body.1).abs() < 1.0 {
        (0.0, 1.0)
    } else {
        (0.0, -1.0)
    };
    let (nx, ny) = (-dy, dx);
    let tri = [
        (p.0 + nx * -4.0, p.1 + ny * -4.0),
        (p.0 + dx * 6.0, p.1 + dy * 6.0),
        (p.0 + nx * 4.0, p.1 + ny * 4.0),
    ];
    g.poly(&tri, false, None, g.theme.stroke);
}

/// Index label (`0`, `1`, …) for data ports in a column, dim and small.
pub fn index_labels(g: &Gfx, v: &PartView, ports: std::ops::Range<usize>, body: Rect4) {
    for (k, i) in ports.enumerate() {
        port_label(g, v, i, body, &k.to_string());
    }
}

/// Label for a control port that hangs off the body (select, enable, …): drawn just outside it,
/// on the side facing away from the body's centre.
pub fn control_label(g: &Gfx, v: &PartView, i: usize, body: Rect4, s: &str) {
    let Some(p) = port_at(v, i) else { return };
    let out = if p.1 >= (body.1 + body.3) / 2.0 {
        1.0
    } else {
        -1.0
    };
    inside_text(g, (p.0, p.1 + out * 5.0), (0.0, out), s, 7.0, g.theme.dim);
}

/// Like [`port_label`], but ports on the top/bottom edge get their label *outside* the body, where
/// it cannot collide with the side labels of small parts.
pub fn edge_label(g: &Gfx, v: &PartView, i: usize, body: Rect4, s: &str) {
    match port_at(v, i) {
        Some(p) if (p.1 - body.1).abs() < 1.0 || (p.1 - body.3).abs() < 1.0 => {
            control_label(g, v, i, body, s)
        }
        _ => port_label(g, v, i, body, s),
    }
}
