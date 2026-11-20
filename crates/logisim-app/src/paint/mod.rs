//! Drawing parts. A [`PartPainter`] draws one kind of part in its own east-facing local
//! coordinates; [`Gfx`] handles rotation, position and view transform. Unknown parts fall
//! back to a labelled box, so a library without a painter still shows up.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Vec2};
use logisim_core::{Direction, Port, Value};

pub mod gates;
pub mod ttl;
pub mod wiring;
use logisim_format::ComponentDef;

// ---- theme ---------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Clone)]
pub struct Theme {
    pub bg: Color32,
    pub grid: Color32,
    pub grid_major: Color32,
    pub stroke: Color32,
    pub fill: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub select: Color32,
    pub v_true: Color32,
    pub v_false: Color32,
    pub v_x: Color32,
    pub v_err: Color32,
    pub bus: Color32,
    pub idle: Color32,
}

impl Theme {
    pub fn new(mode: ThemeMode) -> Theme {
        match mode {
            ThemeMode::Dark => Theme {
                bg: Color32::from_rgb(24, 26, 31),
                grid: Color32::from_rgb(38, 41, 48),
                grid_major: Color32::from_rgb(50, 54, 63),
                stroke: Color32::from_rgb(214, 218, 226),
                fill: Color32::from_rgb(38, 42, 52),
                text: Color32::from_rgb(226, 229, 235),
                dim: Color32::from_rgb(130, 136, 148),
                select: Color32::from_rgb(86, 156, 255),
                v_true: Color32::from_rgb(60, 220, 110),
                v_false: Color32::from_rgb(28, 130, 70),
                v_x: Color32::from_rgb(90, 120, 200),
                v_err: Color32::from_rgb(240, 80, 80),
                bus: Color32::from_rgb(120, 170, 255),
                idle: Color32::from_rgb(150, 156, 168),
            },
            ThemeMode::Light => Theme {
                bg: Color32::from_rgb(250, 250, 252),
                grid: Color32::from_rgb(232, 234, 240),
                grid_major: Color32::from_rgb(214, 218, 228),
                stroke: Color32::from_rgb(40, 44, 54),
                fill: Color32::from_rgb(255, 255, 255),
                text: Color32::from_rgb(30, 34, 44),
                dim: Color32::from_rgb(120, 126, 140),
                select: Color32::from_rgb(20, 110, 235),
                v_true: Color32::from_rgb(0, 170, 70),
                v_false: Color32::from_rgb(10, 100, 50),
                v_x: Color32::from_rgb(70, 100, 190),
                v_err: Color32::from_rgb(220, 40, 40),
                bus: Color32::from_rgb(30, 80, 200),
                idle: Color32::from_rgb(120, 126, 140),
            },
        }
    }

    pub fn value_color(&self, v: Option<Value>) -> Color32 {
        let Some(v) = v else { return self.idle };
        if v.is_error() {
            self.v_err
        } else if v.width().get() > 1 {
            self.bus
        } else if v == Value::TRUE {
            self.v_true
        } else if v == Value::FALSE {
            self.v_false
        } else {
            self.v_x
        }
    }
}

/// Short text for a value: `1`, `0`, `x`, `E` for one bit; hex when defined, else bits.
pub fn value_text(v: &Value) -> String {
    let w = v.width().get();
    if w == 0 {
        return "-".into();
    }
    if w == 1 {
        return v.to_binary_string();
    }
    match v.to_u64() {
        Some(n) => format!("{n:X}"),
        None => v.to_binary_string(),
    }
}

// ---- view + gfx ------------------------------------------------------------------------------

/// World ↔ screen mapping (world units are Logisim pixels on a 10-unit grid).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub origin: Vec2,
    pub zoom: f32,
}

impl View {
    pub fn to_screen(self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.origin.x + x * self.zoom, self.origin.y + y * self.zoom)
    }
    pub fn to_world(self, p: Pos2) -> (f32, f32) {
        (
            (p.x - self.origin.x) / self.zoom,
            (p.y - self.origin.y) / self.zoom,
        )
    }
}

pub fn rotate_f(d: Direction, x: f32, y: f32) -> (f32, f32) {
    match d {
        Direction::East => (x, y),
        Direction::West => (-x, -y),
        Direction::North => (y, -x),
        Direction::South => (-y, x),
    }
}

/// Drawing context for one placed part.
pub struct Gfx<'a> {
    pub painter: &'a egui::Painter,
    pub view: View,
    pub theme: &'a Theme,
    pub at: (i32, i32),
    pub facing: Direction,
    /// 1.0 normal; below 1 for ghost previews.
    pub alpha: f32,
}

impl Gfx<'_> {
    fn col(&self, c: Color32) -> Color32 {
        if self.alpha >= 1.0 {
            c
        } else {
            c.gamma_multiply(self.alpha)
        }
    }
    /// Local east-facing point → screen.
    pub fn p(&self, x: f32, y: f32) -> Pos2 {
        let (rx, ry) = rotate_f(self.facing, x, y);
        self.view
            .to_screen(self.at.0 as f32 + rx, self.at.1 as f32 + ry)
    }
    fn sw(&self, w: f32) -> f32 {
        (w * self.view.zoom).max(1.0)
    }
    pub fn stroke(&self, color: Color32) -> Stroke {
        Stroke::new(self.sw(1.6), self.col(color))
    }
    pub fn line(&self, a: (f32, f32), b: (f32, f32), color: Color32) {
        self.painter
            .line_segment([self.p(a.0, a.1), self.p(b.0, b.1)], self.stroke(color));
    }
    /// Polyline / polygon from local points.
    pub fn poly(&self, pts: &[(f32, f32)], closed: bool, fill: Option<Color32>, color: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|(x, y)| self.p(*x, *y)).collect();
        if let (true, Some(f)) = (closed, fill) {
            self.painter.add(Shape::convex_polygon(
                pts.clone(),
                self.col(f),
                Stroke::NONE,
            ));
        }
        if closed {
            self.painter
                .add(Shape::closed_line(pts, self.stroke(color)));
        } else {
            self.painter.add(Shape::line(pts, self.stroke(color)));
        }
    }
    pub fn circle(&self, c: (f32, f32), r: f32, fill: Option<Color32>, color: Color32) {
        let center = self.p(c.0, c.1);
        if let Some(f) = fill {
            self.painter
                .circle_filled(center, r * self.view.zoom, self.col(f));
        }
        self.painter
            .circle_stroke(center, r * self.view.zoom, self.stroke(color));
    }
    /// Axis-aligned (in local space) rectangle.
    pub fn rect(&self, a: (f32, f32), b: (f32, f32), fill: Option<Color32>, color: Color32) {
        self.poly(
            &[(a.0, a.1), (b.0, a.1), (b.0, b.1), (a.0, b.1)],
            true,
            fill,
            color,
        );
    }
    /// Upright text (never rotated) anchored at a local point.
    pub fn text(&self, at: (f32, f32), anchor: Align2, s: &str, size: f32, color: Color32) {
        let fid = FontId::proportional((size * self.view.zoom).clamp(6.0, 60.0));
        self.painter
            .text(self.p(at.0, at.1), anchor, s, fid, self.col(color));
    }
    pub fn bubble(&self, c: (f32, f32)) {
        self.circle(c, 4.5, Some(self.theme.fill), self.theme.stroke);
    }
}

/// Everything a painter may need about one placed part.
pub struct PartView<'a> {
    pub def: &'a ComponentDef,
    pub name: &'a str,
    /// Ports in local (east-facing) coordinates.
    pub ports: &'a [Port],
    /// Net value at each port (live simulation), if any.
    pub port_vals: &'a [Option<Value>],
    pub selected: bool,
    /// Port names for subcircuit instances.
    pub port_names: &'a [String],
}

impl PartView<'_> {
    pub fn int(&self, key: &str, default: i64) -> i64 {
        self.def
            .options
            .int(key, i64::MIN, i64::MAX)
            .ok()
            .flatten()
            .unwrap_or(default)
    }
    pub fn boolean(&self, key: &str) -> bool {
        self.def
            .options
            .boolean(key)
            .ok()
            .flatten()
            .unwrap_or(false)
    }
    pub fn string(&self, key: &str) -> String {
        self.def
            .options
            .string(key)
            .ok()
            .flatten()
            .unwrap_or("")
            .to_string()
    }
    pub fn port_color(&self, th: &Theme, i: usize) -> Color32 {
        th.value_color(self.port_vals.get(i).copied().flatten())
    }
}

pub trait PartPainter {
    fn paint(&self, g: &Gfx, v: &PartView);
    /// Local bounding box (min x, min y, max x, max y) used for hit testing and selection.
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        port_bounds(v, 6.0)
    }
}

pub fn port_bounds(v: &PartView, pad: f32) -> (f32, f32, f32, f32) {
    let (mut x0, mut y0, mut x1, mut y1) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for p in v.ports {
        x0 = x0.min(p.offset.x as f32);
        x1 = x1.max(p.offset.x as f32);
        y0 = y0.min(p.offset.y as f32);
        y1 = y1.max(p.offset.y as f32);
    }
    (x0 - pad, y0 - pad, x1 + pad, y1 + pad)
}

pub fn union(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

/// Draw the standard connection dots with live value colours.
fn port_dots(g: &Gfx, v: &PartView) {
    for (i, p) in v.ports.iter().enumerate() {
        let c = v.port_color(g.theme, i);
        let at = (p.offset.x as f32, p.offset.y as f32);
        g.circle(at, 2.2, Some(c), c);
    }
}

/// Draw one part with its painter, plus selection highlight and port dots.
pub fn paint_part(g: &Gfx, v: &PartView) {
    let painter = painter_for(&v.def.library, &v.def.part);
    painter.paint(g, v);
    port_dots(g, v);
    if v.selected {
        let b = painter.bounds(v);
        g.rect((b.0, b.1), (b.2, b.3), None, g.theme.select);
    }
}

/// World-space bounding box of a placed part (for hit tests and box select).
pub fn world_bounds(def: &ComponentDef, v: &PartView) -> Rect {
    let b = painter_for(&def.library, &def.part).bounds(v);
    let corners = [(b.0, b.1), (b.2, b.1), (b.2, b.3), (b.0, b.3)];
    let mut r = Rect::NOTHING;
    for (x, y) in corners {
        let (rx, ry) = rotate_f(def.facing, x, y);
        r.extend_with(Pos2::new(def.at.0 as f32 + rx, def.at.1 as f32 + ry));
    }
    r
}

// ---- registry --------------------------------------------------------------------------------

/// Painters of one library: given a part name, return its painter (or `None`).
pub type LibraryPainters = fn(&str) -> Option<&'static dyn PartPainter>;

/// One line per library: `("Library name", module::painter)`. Parts without a painter (or whose
/// library is not listed) are drawn as a labelled box.
const LIBRARIES: &[(&str, LibraryPainters)] = &[
    ("Gates", gates::painter),
    ("Wiring", wiring::painter),
    ("TTL", ttl::painter),
];

pub fn painter_for(library: &str, part: &str) -> &'static dyn PartPainter {
    LIBRARIES
        .iter()
        .find(|(l, _)| *l == library)
        .and_then(|(_, f)| f(part))
        .unwrap_or(&BoxPainter)
}

// ---- generic box (fallback and subcircuits) --------------------------------------------------

pub struct BoxPainter;

impl PartPainter for BoxPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let b = port_bounds(v, 0.0);
        let (x0, x1) = if v.ports.is_empty() {
            (-20.0, 20.0)
        } else {
            (b.0 + 6.0, b.2 - 6.0)
        };
        let (y0, y1) = if v.ports.is_empty() {
            (-10.0, 10.0)
        } else {
            (b.1 - 10.0, b.3 + 10.0)
        };
        let (x0, x1) = if x1 - x0 < 24.0 {
            (x0 - 12.0, x1 + 12.0)
        } else {
            (x0, x1)
        };
        g.rect((x0, y0), (x1, y1), Some(g.theme.fill), g.theme.stroke);
        for p in v.ports {
            let (px, py) = (p.offset.x as f32, p.offset.y as f32);
            let ex = if px <= x0 + 1.0 {
                x0
            } else if px >= x1 - 1.0 {
                x1
            } else {
                px
            };
            g.line((px, py), (ex, py), g.theme.stroke);
        }
        for (i, name) in v.port_names.iter().enumerate() {
            if let Some(p) = v.ports.get(i) {
                let left = (p.offset.x as f32) <= (x0 + x1) / 2.0;
                let (ax, anchor) = if left {
                    (x0 + 3.0, Align2::LEFT_CENTER)
                } else {
                    (x1 - 3.0, Align2::RIGHT_CENTER)
                };
                g.text((ax, p.offset.y as f32), anchor, name, 8.0, g.theme.dim);
            }
        }
        g.text(
            ((x0 + x1) / 2.0, y0 - 6.0),
            Align2::CENTER_BOTTOM,
            v.name,
            9.0,
            g.theme.text,
        );
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let b = port_bounds(v, 0.0);
        if v.ports.is_empty() {
            (-24.0, -14.0, 24.0, 14.0)
        } else {
            (b.0, b.1 - 12.0, b.2, b.3 + 12.0)
        }
    }
}
