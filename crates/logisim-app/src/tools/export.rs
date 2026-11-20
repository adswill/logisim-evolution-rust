//! Render a circuit to an image: SVG (vector) or PNG (raster).
//!
//! The circuit is drawn with the same part painters as the editor, into a headless egui context;
//! the resulting shapes are turned into a small list of primitives ([`Scene`]) that both the SVG
//! writer and the PNG rasteriser understand.
//!
//! Entry points (no `App` needed):
//! * [`render`]`(project, circuit, &ExportOptions) -> Result<Scene, String>`
//! * [`to_svg`]`(&Scene) -> String`, [`to_png`]`(&Scene) -> Result<Vec<u8>, String>`
//! * [`export_svg`] / [`export_png`] do both steps.

use crate::app::{make_info, Info};
use crate::paint::{self, Gfx, PartView, Theme, ThemeMode, View};
use eframe::egui::{self, epaint, Color32, Pos2, Rect, Shape, Vec2};
use logisim_format::{CircuitDef, ProjectFile};

#[derive(Clone, Debug)]
pub struct ExportOptions {
    /// Pixels per world unit. 1.0 for SVG, 2.0 gives a crisp PNG.
    pub scale: f32,
    /// Empty border around the circuit, in world units.
    pub margin: f32,
    /// Dark theme instead of the print-friendly light one.
    pub dark: bool,
    /// Paint the theme background (otherwise transparent).
    pub background: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            scale: 1.0,
            margin: 20.0,
            dark: false,
            background: true,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Prim {
    Poly {
        pts: Vec<Pos2>,
        closed: bool,
        fill: Option<Color32>,
        stroke: Option<(f32, Color32)>,
    },
    Circle {
        c: Pos2,
        r: f32,
        fill: Option<Color32>,
        stroke: Option<(f32, Color32)>,
    },
    Text {
        /// (character, x, baseline y)
        glyphs: Vec<(char, f32, f32)>,
        size: f32,
        mono: bool,
        color: Color32,
    },
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    pub background: Option<Color32>,
    pub prims: Vec<Prim>,
}

fn opt_stroke(s: epaint::Stroke) -> Option<(f32, Color32)> {
    (s.width > 0.0 && s.color != Color32::TRANSPARENT).then_some((s.width, s.color))
}

fn opt_fill(c: Color32) -> Option<Color32> {
    (c != Color32::TRANSPARENT).then_some(c)
}

fn flatten(shape: &Shape, out: &mut Vec<Prim>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
        Shape::LineSegment { points, stroke } => {
            if let Some(s) = opt_stroke(*stroke) {
                out.push(Prim::Poly {
                    pts: points.to_vec(),
                    closed: false,
                    fill: None,
                    stroke: Some(s),
                });
            }
        }
        Shape::Path(p) => {
            let stroke = match &p.stroke.color {
                epaint::ColorMode::Solid(c)
                    if p.stroke.width > 0.0 && *c != Color32::TRANSPARENT =>
                {
                    Some((p.stroke.width, *c))
                }
                _ => None,
            };
            out.push(Prim::Poly {
                pts: p.points.clone(),
                closed: p.closed,
                fill: opt_fill(p.fill),
                stroke,
            });
        }
        Shape::Circle(c) => out.push(Prim::Circle {
            c: c.center,
            r: c.radius,
            fill: opt_fill(c.fill),
            stroke: opt_stroke(c.stroke),
        }),
        Shape::Rect(r) => {
            let (a, b) = (r.rect.min, r.rect.max);
            out.push(Prim::Poly {
                pts: vec![a, Pos2::new(b.x, a.y), b, Pos2::new(a.x, b.y)],
                closed: true,
                fill: opt_fill(r.fill),
                stroke: opt_stroke(r.stroke),
            });
        }
        Shape::Text(t) => {
            let Some(sec) = t.galley.job.sections.first() else {
                return;
            };
            let mut color = t.override_text_color.unwrap_or(sec.format.color);
            if color == Color32::PLACEHOLDER {
                color = t.fallback_color;
            }
            let mono = matches!(sec.format.font_id.family, egui::FontFamily::Monospace);
            for row in &t.galley.rows {
                let glyphs: Vec<(char, f32, f32)> = row
                    .row
                    .glyphs
                    .iter()
                    .map(|g| {
                        (
                            g.chr,
                            t.pos.x + row.pos.x + g.pos.x,
                            t.pos.y + row.pos.y + g.pos.y + g.font_ascent,
                        )
                    })
                    .collect();
                if !glyphs.is_empty() {
                    out.push(Prim::Text {
                        glyphs,
                        size: sec.format.font_id.size,
                        mono,
                        color,
                    });
                }
            }
        }
        _ => {}
    }
}

fn is_junction(c: &CircuitDef, p: (i32, i32), port_ends: &[(i32, i32)]) -> bool {
    let mut ends = port_ends.iter().filter(|q| **q == p).count();
    for w in &c.wires {
        if w.a == p || w.b == p {
            ends += 1;
        } else if (w.a.0 == w.b.0
            && w.a.0 == p.0
            && p.1 > w.a.1.min(w.b.1)
            && p.1 < w.a.1.max(w.b.1))
            || (w.a.1 == w.b.1 && w.a.1 == p.1 && p.0 > w.a.0.min(w.b.0) && p.0 < w.a.0.max(w.b.0))
        {
            ends += 2; // a wire passing straight through counts as two meeting there
        }
    }
    ends >= 3
}

/// Draw `circuit` into a [`Scene`].
pub fn render(project: &ProjectFile, circuit: &str, opts: &ExportOptions) -> Result<Scene, String> {
    let c = project
        .circuit(circuit)
        .ok_or_else(|| format!("There is no circuit named '{circuit}'."))?;
    let infos: Vec<Info> = c.components.iter().map(|d| make_info(project, d)).collect();
    let no_vals: Vec<Option<logisim_core::Value>> = vec![];
    let view_of = |i: usize| PartView {
        def: &c.components[i],
        name: &infos[i].name,
        ports: &infos[i].ports,
        port_vals: &no_vals,
        selected: false,
        port_names: &infos[i].port_names,
    };

    // world bounds
    let mut b = Rect::NOTHING;
    for i in 0..c.components.len() {
        let r = paint::world_bounds(&c.components[i], &view_of(i));
        b.extend_with(r.min);
        b.extend_with(r.max);
    }
    for w in &c.wires {
        b.extend_with(Pos2::new(w.a.0 as f32, w.a.1 as f32));
        b.extend_with(Pos2::new(w.b.0 as f32, w.b.1 as f32));
    }
    if !b.is_finite() {
        return Err(format!(
            "Circuit '{circuit}' is empty, so there is nothing to export."
        ));
    }
    let scale = opts.scale.clamp(0.25, 16.0);
    let b = b.expand(opts.margin);
    let size = b.size() * scale;
    if size.x * size.y > 150_000_000.0 {
        return Err("The image would be too large; lower the scale.".into());
    }
    let view = View {
        origin: Vec2::new(-b.min.x * scale, -b.min.y * scale),
        zoom: scale,
    };

    let th = Theme::new(if opts.dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    });
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
        ..Default::default()
    };
    let port_ends: Vec<(i32, i32)> = infos.iter().flat_map(|i| i.abs.iter().copied()).collect();
    let mut out = ctx.run_pass(input, |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let wire = epaint::Stroke::new((2.0 * scale).max(1.0), th.stroke);
        for w in &c.wires {
            painter.line_segment(
                [
                    view.to_screen(w.a.0 as f32, w.a.1 as f32),
                    view.to_screen(w.b.0 as f32, w.b.1 as f32),
                ],
                wire,
            );
        }
        let mut done = std::collections::HashSet::new();
        for w in &c.wires {
            for p in [w.a, w.b] {
                if done.insert(p) && is_junction(c, p, &port_ends) {
                    painter.circle_filled(
                        view.to_screen(p.0 as f32, p.1 as f32),
                        3.0 * scale,
                        th.stroke,
                    );
                }
            }
        }
        for (i, d) in c.components.iter().enumerate() {
            let g = Gfx {
                painter: &painter,
                view,
                theme: &th,
                at: d.at,
                facing: d.facing,
                mirror: d.mirror,
                alpha: 1.0,
                port_px: 2.0,
            };
            paint::paint_part(&g, &view_of(i));
        }
    });
    let mut prims = vec![];
    for cs in &out.shapes {
        flatten(&cs.shape, &mut prims);
    }
    out.textures_delta.clear();
    Ok(Scene {
        width: size.x.ceil(),
        height: size.y.ceil(),
        background: opts.background.then_some(th.bg),
        prims,
    })
}

// ---- SVG ----------------------------------------------------------------------------------------

fn css(c: Color32) -> String {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn alpha(c: Color32) -> f32 {
    c.a() as f32 / 255.0
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn paint_attrs(fill: Option<Color32>, stroke: Option<(f32, Color32)>) -> String {
    let mut s = match fill {
        Some(f) => format!(
            " fill=\"{}\"{}",
            css(f),
            if alpha(f) < 1.0 {
                format!(" fill-opacity=\"{:.3}\"", alpha(f))
            } else {
                String::new()
            }
        ),
        None => " fill=\"none\"".into(),
    };
    if let Some((w, c)) = stroke {
        s += &format!(" stroke=\"{}\" stroke-width=\"{:.2}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"", css(c), w);
        if alpha(c) < 1.0 {
            s += &format!(" stroke-opacity=\"{:.3}\"", alpha(c));
        }
    }
    s
}

pub fn to_svg(scene: &Scene) -> String {
    let (w, h) = (scene.width, scene.height);
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n"
    );
    if let Some(bg) = scene.background {
        s += &format!(
            "<rect width=\"{w}\" height=\"{h}\" fill=\"{}\"/>\n",
            css(bg)
        );
    }
    for p in &scene.prims {
        match p {
            Prim::Poly {
                pts,
                closed,
                fill,
                stroke,
            } => {
                if pts.len() < 2 {
                    continue;
                }
                let list: Vec<String> = pts
                    .iter()
                    .map(|p| format!("{:.2},{:.2}", p.x, p.y))
                    .collect();
                let tag = if *closed { "polygon" } else { "polyline" };
                s += &format!(
                    "<{tag} points=\"{}\"{}/>\n",
                    list.join(" "),
                    paint_attrs(*fill, *stroke)
                );
            }
            Prim::Circle { c, r, fill, stroke } => {
                s += &format!(
                    "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\"{}/>\n",
                    c.x,
                    c.y,
                    r,
                    paint_attrs(*fill, *stroke)
                );
            }
            Prim::Text {
                glyphs,
                size,
                mono,
                color,
            } => {
                let (_, x, y) = glyphs[0];
                let text: String = glyphs.iter().map(|g| g.0).collect();
                let fam = if *mono { "monospace" } else { "sans-serif" };
                s += &format!(
                    "<text x=\"{x:.2}\" y=\"{y:.2}\" font-size=\"{size:.2}\" font-family=\"{fam}\" fill=\"{}\" xml:space=\"preserve\">{}</text>\n",
                    css(*color),
                    esc(&text)
                );
            }
        }
    }
    s + "</svg>\n"
}

// ---- PNG ----------------------------------------------------------------------------------------

fn skia_color(c: Color32) -> tiny_skia::Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    tiny_skia::Color::from_rgba8(r, g, b, a)
}

pub fn to_png(scene: &Scene) -> Result<Vec<u8>, String> {
    use ab_glyph::{Font, FontRef, ScaleFont};
    use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

    let mut pm = Pixmap::new(scene.width.max(1.0) as u32, scene.height.max(1.0) as u32)
        .ok_or("The image is too large.")?;
    if let Some(bg) = scene.background {
        pm.fill(skia_color(bg));
    }
    let fonts = egui::FontDefinitions::default();
    let load = |name: &str| -> Option<FontRef<'_>> {
        let d = fonts.font_data.get(name)?;
        FontRef::try_from_slice((*d.font).as_ref()).ok()
    };
    let (prop, mono) = (load("Ubuntu-Light"), load("Hack"));

    let paint_of = |c: Color32| {
        let mut p = Paint::default();
        p.set_color(skia_color(c));
        p.anti_alias = true;
        p
    };
    let stroke_of = |w: f32| Stroke {
        width: w,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    };
    for p in &scene.prims {
        match p {
            Prim::Poly {
                pts,
                closed,
                fill,
                stroke,
            } => {
                let mut pb = PathBuilder::new();
                for (i, q) in pts.iter().enumerate() {
                    if i == 0 {
                        pb.move_to(q.x, q.y);
                    } else {
                        pb.line_to(q.x, q.y);
                    }
                }
                if *closed {
                    pb.close();
                }
                if let Some(path) = pb.finish() {
                    if let Some(f) = fill {
                        pm.fill_path(
                            &path,
                            &paint_of(*f),
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                    if let Some((w, c)) = stroke {
                        pm.stroke_path(
                            &path,
                            &paint_of(*c),
                            &stroke_of(*w),
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
            Prim::Circle { c, r, fill, stroke } => {
                if let Some(path) = PathBuilder::from_circle(c.x, c.y, *r) {
                    if let Some(f) = fill {
                        pm.fill_path(
                            &path,
                            &paint_of(*f),
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                    if let Some((w, col)) = stroke {
                        pm.stroke_path(
                            &path,
                            &paint_of(*col),
                            &stroke_of(*w),
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
            Prim::Text {
                glyphs,
                size,
                mono: m,
                color,
            } => {
                let Some(font) = (if *m { mono.as_ref() } else { prop.as_ref() }).or(prop.as_ref())
                else {
                    continue;
                };
                let scaled = font.as_scaled(*size);
                let [cr, cg, cb, ca] = color.to_srgba_unmultiplied();
                let (w, h) = (pm.width() as i32, pm.height() as i32);
                for (ch, x, y) in glyphs {
                    let g = scaled.scaled_glyph(*ch).clone();
                    let mut g = g;
                    g.position = ab_glyph::point(*x, *y);
                    if let Some(og) = font.outline_glyph(g) {
                        let bb = og.px_bounds();
                        og.draw(|gx, gy, cov| {
                            let (px, py) =
                                (bb.min.x as i32 + gx as i32, bb.min.y as i32 + gy as i32);
                            if px < 0 || py < 0 || px >= w || py >= h {
                                return;
                            }
                            let a = cov * ca as f32 / 255.0;
                            let i = ((py * w + px) * 4) as usize;
                            let d = pm.data_mut();
                            for (k, s) in [cr, cg, cb].into_iter().enumerate() {
                                d[i + k] = (d[i + k] as f32 * (1.0 - a) + s as f32 * a * 1.0)
                                    .round() as u8;
                            }
                            d[i + 3] = (d[i + 3] as f32 * (1.0 - a) + 255.0 * a).round() as u8;
                        });
                    }
                }
            }
        }
    }
    pm.encode_png().map_err(|e| e.to_string())
}

pub fn export_svg(
    project: &ProjectFile,
    circuit: &str,
    opts: &ExportOptions,
) -> Result<String, String> {
    render(project, circuit, opts).map(|s| to_svg(&s))
}

pub fn export_png(
    project: &ProjectFile,
    circuit: &str,
    opts: &ExportOptions,
) -> Result<Vec<u8>, String> {
    render(project, circuit, opts).and_then(|s| to_png(&s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ProjectFile {
        crate::examples::half_adder().project
    }

    #[test]
    fn svg_contains_shapes_and_text() {
        let p = sample();
        let name = p.main.clone();
        let svg = export_svg(&p, &name, &ExportOptions::default()).unwrap();
        assert!(svg.starts_with("<svg "));
        assert!(svg.contains("<polyline") || svg.contains("<polygon"));
        assert!(svg.contains("<circle"), "junction dots expected");
        assert!(svg.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn png_has_signature_and_scales() {
        let p = sample();
        let name = p.main.clone();
        let small = export_png(&p, &name, &ExportOptions::default()).unwrap();
        assert_eq!(&small[..8], b"\x89PNG\r\n\x1a\n");
        let big = render(
            &p,
            &name,
            &ExportOptions {
                scale: 2.0,
                ..Default::default()
            },
        )
        .unwrap();
        let one = render(&p, &name, &ExportOptions::default()).unwrap();
        assert!((big.width - one.width * 2.0).abs() <= 2.0);
    }

    #[test]
    fn empty_and_missing_circuits_are_errors() {
        let mut p = ProjectFile::new("t", "main");
        p.circuits.push(CircuitDef {
            name: "main".into(),
            interface: vec![],
            components: vec![],
            wires: vec![],
        });
        assert!(export_svg(&p, "main", &Default::default())
            .unwrap_err()
            .contains("empty"));
        assert!(export_svg(&p, "zzz", &Default::default()).is_err());
    }
}
