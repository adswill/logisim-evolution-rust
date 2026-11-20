//! Drawing parts. A [`PartPainter`] draws one kind of part in its own east-facing local
//! coordinates; [`Gfx`] handles rotation, position and view transform. Unknown parts fall
//! back to a labelled box, so a library without a painter still shows up.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Vec2};
use logisim_core::{Direction, Port, PortKind, Value};
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
    pub fn to_screen(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.origin.x + x * self.zoom, self.origin.y + y * self.zoom)
    }
    pub fn to_world(&self, p: Pos2) -> (f32, f32) {
        ((p.x - self.origin.x) / self.zoom, (p.y - self.origin.y) / self.zoom)
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
        self.view.to_screen(self.at.0 as f32 + rx, self.at.1 as f32 + ry)
    }
    fn sw(&self, w: f32) -> f32 {
        (w * self.view.zoom).max(1.0)
    }
    pub fn stroke(&self, color: Color32) -> Stroke {
        Stroke::new(self.sw(1.6), self.col(color))
    }
    pub fn line(&self, a: (f32, f32), b: (f32, f32), color: Color32) {
        self.painter.line_segment([self.p(a.0, a.1), self.p(b.0, b.1)], self.stroke(color));
    }
    /// Polyline / polygon from local points.
    pub fn poly(&self, pts: &[(f32, f32)], closed: bool, fill: Option<Color32>, color: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|(x, y)| self.p(*x, *y)).collect();
        if let (true, Some(f)) = (closed, fill) {
            self.painter.add(Shape::convex_polygon(pts.clone(), self.col(f), Stroke::NONE));
        }
        if closed {
            self.painter.add(Shape::closed_line(pts, self.stroke(color)));
        } else {
            self.painter.add(Shape::line(pts, self.stroke(color)));
        }
    }
    pub fn circle(&self, c: (f32, f32), r: f32, fill: Option<Color32>, color: Color32) {
        let center = self.p(c.0, c.1);
        if let Some(f) = fill {
            self.painter.circle_filled(center, r * self.view.zoom, self.col(f));
        }
        self.painter.circle_stroke(center, r * self.view.zoom, self.stroke(color));
    }
    /// Axis-aligned (in local space) rectangle.
    pub fn rect(&self, a: (f32, f32), b: (f32, f32), fill: Option<Color32>, color: Color32) {
        self.poly(&[(a.0, a.1), (b.0, a.1), (b.0, b.1), (a.0, b.1)], true, fill, color);
    }
    /// Upright text (never rotated) anchored at a local point.
    pub fn text(&self, at: (f32, f32), anchor: Align2, s: &str, size: f32, color: Color32) {
        let fid = FontId::proportional((size * self.view.zoom).clamp(6.0, 60.0));
        self.painter.text(self.p(at.0, at.1), anchor, s, fid, self.col(color));
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
    fn int(&self, key: &str, default: i64) -> i64 {
        self.def.options.int(key, i64::MIN, i64::MAX).ok().flatten().unwrap_or(default)
    }
    fn boolean(&self, key: &str) -> bool {
        self.def.options.boolean(key).ok().flatten().unwrap_or(false)
    }
    fn string(&self, key: &str) -> String {
        self.def.options.string(key).ok().flatten().unwrap_or("").to_string()
    }
    fn port_color(&self, th: &Theme, i: usize) -> Color32 {
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

fn union(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
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

pub fn painter_for(library: &str, part: &str) -> &'static dyn PartPainter {
    match (library, part) {
        ("Gates", "AND Gate") => &GatePainter { shape: GateShape::And, neg: false },
        ("Gates", "NAND Gate") => &GatePainter { shape: GateShape::And, neg: true },
        ("Gates", "OR Gate") => &GatePainter { shape: GateShape::Or, neg: false },
        ("Gates", "NOR Gate") => &GatePainter { shape: GateShape::Or, neg: true },
        ("Gates", "XOR Gate") => &GatePainter { shape: GateShape::Xor, neg: false },
        ("Gates", "XNOR Gate") => &GatePainter { shape: GateShape::Xor, neg: true },
        ("Gates", "NOT Gate") => &BufferPainter { inverting: true, controlled: false },
        ("Gates", "Buffer") => &BufferPainter { inverting: false, controlled: false },
        ("Gates", "Controlled Buffer") => &BufferPainter { inverting: false, controlled: true },
        ("Gates", "Controlled Inverter") => &BufferPainter { inverting: true, controlled: true },
        ("Wiring", "Pin") => &PinPainter,
        ("Wiring", "Clock") => &ClockPainter,
        ("Wiring", "Splitter") => &SplitterPainter,
        ("Wiring", "Tunnel") => &TunnelPainter,
        ("Wiring", "Pull Resistor") => &PullPainter,
        ("Wiring", "Constant") => &ConstantPainter,
        ("Wiring", "Probe") => &ProbePainter,
        ("Wiring", "Power") => &PowerPainter,
        ("Wiring", "Ground") => &GroundPainter,
        ("Wiring", "Transistor") => &SwitchPainter { gates: 1 },
        ("Wiring", "Transmission Gate") => &SwitchPainter { gates: 2 },
        ("Wiring", "Bit Extender") => &ExtenderPainter,
        _ => &BoxPainter,
    }
}

// ---- generic box (fallback and subcircuits) --------------------------------------------------

struct BoxPainter;

impl PartPainter for BoxPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let b = port_bounds(v, 0.0);
        let (x0, x1) = if v.ports.is_empty() { (-20.0, 20.0) } else { (b.0 + 6.0, b.2 - 6.0) };
        let (y0, y1) = if v.ports.is_empty() { (-10.0, 10.0) } else { (b.1 - 10.0, b.3 + 10.0) };
        let (x0, x1) = if x1 - x0 < 24.0 { (x0 - 12.0, x1 + 12.0) } else { (x0, x1) };
        g.rect((x0, y0), (x1, y1), Some(g.theme.fill), g.theme.stroke);
        for p in v.ports {
            let (px, py) = (p.offset.x as f32, p.offset.y as f32);
            let ex = if px <= x0 + 1.0 { x0 } else if px >= x1 - 1.0 { x1 } else { px };
            g.line((px, py), (ex, py), g.theme.stroke);
        }
        for (i, name) in v.port_names.iter().enumerate() {
            if let Some(p) = v.ports.get(i) {
                let left = (p.offset.x as f32) <= (x0 + x1) / 2.0;
                let (ax, anchor) = if left { (x0 + 3.0, Align2::LEFT_CENTER) } else { (x1 - 3.0, Align2::RIGHT_CENTER) };
                g.text((ax, p.offset.y as f32), anchor, name, 8.0, g.theme.dim);
            }
        }
        g.text(((x0 + x1) / 2.0, y0 - 6.0), Align2::CENTER_BOTTOM, v.name, 9.0, g.theme.text);
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

// ---- gates -----------------------------------------------------------------------------------

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
            (u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p1.0, u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p1.1)
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
        let fill = if self.shape == GateShape::And { Some(th.fill) } else { None };
        g.poly(&body, true, fill, th.stroke);
        if self.shape == GateShape::Xor {
            g.poly(&quad((back - 8.0, h), (back + 6.0, 0.0), (back - 8.0, -h), 10), false, None, th.stroke);
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
        union((back - 10.0, -h - 2.0, tip.max(0.0) + 2.0, h + 2.0), port_bounds(v, 4.0))
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
        g.poly(&[(base, -9.0), (base, 9.0), (tip, 0.0)], true, Some(th.fill), th.stroke);
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

// ---- wiring ----------------------------------------------------------------------------------

fn digits(width: i64) -> f32 {
    ((width as f32) / 4.0).ceil().max(1.0)
}

struct PinPainter;
impl PartPainter for PinPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let width = v.int("width", 1);
        let output = v.boolean("output");
        let w = if width <= 1 { 20.0 } else { 10.0 * digits(width) + 8.0 };
        let live = v.port_vals.first().copied().flatten();
        let (txt, color) = if output {
            (live.map_or("?".to_string(), |x| value_text(&x)), th.value_color(live))
        } else {
            let val = v.int("value", 0) as u64;
            let n = logisim_core::Value::known(logisim_core::BitWidth::new(width.clamp(1, 64) as u8), val);
            (value_text(&n), th.value_color(Some(n)))
        };
        if output {
            g.circle((-w / 2.0, 0.0), w / 2.0, Some(th.fill), th.stroke);
        } else {
            g.rect((-w, -10.0), (0.0, 10.0), Some(th.fill), th.stroke);
        }
        g.text((-w / 2.0, 0.0), Align2::CENTER_CENTER, &txt, 11.0, color);
        let label = v.string("label");
        if !label.is_empty() {
            g.text((-w / 2.0, -12.0), Align2::CENTER_BOTTOM, &label, 9.0, th.dim);
        }
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let width = v.int("width", 1);
        let w = if width <= 1 { 20.0 } else { 10.0 * digits(width) + 8.0 };
        (-w - 2.0, -12.0, 4.0, 12.0)
    }
}

struct ClockPainter;
impl PartPainter for ClockPainter {
    fn paint(&self, g: &Gfx, v: &PartView) {
        let th = g.theme;
        let c = v.port_color(th, 0);
        g.rect((-20.0, -10.0), (0.0, 10.0), Some(th.fill), c);
        g.poly(&[(-17.0, 4.0), (-12.0, 4.0), (-12.0, -4.0), (-7.0, -4.0), (-7.0, 4.0), (-3.0, 4.0)], false, None, c);
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
        let (lo, hi) = (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max));
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
            let bits: Vec<usize> = bit_end.iter().enumerate().filter(|(_, e)| **e as usize == i + 1).map(|(b, _)| b).collect();
            let label = match (bits.first(), bits.last()) {
                (Some(a), Some(b)) if a == b => format!("{a}"),
                (Some(a), Some(b)) => format!("{a}-{b}"),
                _ => String::new(),
            };
            g.text((px + 3.0, py - 1.0), Align2::LEFT_BOTTOM, &label, 8.0, th.dim);
        }
        g.text((-3.0, -2.0), Align2::RIGHT_BOTTOM, &v.int("width", 1).to_string(), 8.0, th.dim);
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
        g.poly(&[(0.0, 0.0), (-9.0, -8.0), (-9.0 - w, -8.0), (-9.0 - w, 8.0), (-9.0, 8.0)], true, Some(th.fill), c);
        g.text((-9.0 - w / 2.0, 0.0), Align2::CENTER_CENTER, &name, 10.0, th.text);
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
        let zig = [(0.0, 0.0), (0.0, 3.0), (-4.0, 6.0), (4.0, 10.0), (-4.0, 14.0), (4.0, 18.0), (0.0, 21.0), (0.0, 24.0)];
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
        let n = logisim_core::Value::known(logisim_core::BitWidth::new(width.clamp(1, 64) as u8), val);
        let txt = value_text(&n);
        let w = 7.0 * txt.chars().count().max(1) as f32 + 6.0;
        g.line((-1.0, 0.0), (0.0, 0.0), th.stroke);
        g.rect((-w, -7.0), (-1.0, 7.0), None, th.dim);
        g.text((-w / 2.0 - 0.5, 0.0), Align2::CENTER_CENTER, &txt, 10.0, th.value_color(Some(n)));
    }
    fn bounds(&self, v: &PartView) -> (f32, f32, f32, f32) {
        let width = v.int("width", 1);
        let n = logisim_core::Value::known(logisim_core::BitWidth::new(width.clamp(1, 64) as u8), v.int("value", 0) as u64);
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
        g.text((w / 2.0, 0.0), Align2::CENTER_CENTER, &txt, 11.0, th.value_color(live));
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
        g.poly(&[(-7.0, -5.0), (7.0, -5.0), (0.0, -15.0)], true, Some(th.v_true), th.stroke);
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
        let label = if self.gates == 1 { v.string("type").to_uppercase() } else { "TG".into() };
        g.text((-20.0, 0.0), Align2::CENTER_CENTER, &label, 10.0, th.text);
        for i in 0..self.gates {
            let p = &v.ports[2 + i];
            let py = p.offset.y as f32;
            let edge: f32 = if p.offset.y < 0 { -8.0 } else { 8.0 };
            let p_type = if self.gates == 1 { v.string("type") == "p" } else { i == 0 };
            let (tail, bubble_y) = if p_type { (edge + edge.signum() * 9.0, edge + edge.signum() * 4.5) } else { (edge, 0.0) };
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

/// Kind of port for colouring (inputs of the placed part), used by the canvas for wire snapping.
pub fn is_input(p: &Port) -> bool {
    p.kind != PortKind::Output
}
