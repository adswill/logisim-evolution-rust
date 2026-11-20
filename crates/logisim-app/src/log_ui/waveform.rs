//! The timing diagram: one row per recorded signal, drawn from a [`LogModel`].
//!
//! Colour is never the only cue: unknown values are drawn as a dashed mid-line labelled `x`,
//! errors as a red mid-line labelled `E`, and multi-bit values show their number inside the box.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use logisim_core::Value;
use logisim_log::{model::format_duration, LogModel, Signal};

const ROW_H: f32 = 30.0;
const AXIS_H: f32 = 22.0;
const NAME_W: f32 = 170.0;
const MIN_PX_PER_NS: f64 = 1e-6;

/// Which stretch of time is visible and how it maps to pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeView {
    /// Time (ns) at the left edge of the plot.
    pub start: f64,
    pub px_per_ns: f64,
}

impl TimeView {
    pub fn x_of(&self, t: f64) -> f32 {
        ((t - self.start) * self.px_per_ns) as f32
    }
    pub fn t_of(&self, x: f32) -> f64 {
        self.start + x as f64 / self.px_per_ns
    }
    /// Show `[from, to]` across `width` pixels.
    pub fn fit(from: f64, to: f64, width: f32) -> TimeView {
        let span = (to - from).max(1.0);
        TimeView {
            start: from,
            px_per_ns: (width.max(1.0) as f64 / span).max(MIN_PX_PER_NS),
        }
    }
    /// Zoom by `factor` keeping the time under pixel `x` fixed.
    pub fn zoom(&mut self, factor: f64, x: f32) {
        let t = self.t_of(x);
        self.px_per_ns = (self.px_per_ns * factor).clamp(MIN_PX_PER_NS, 1e3);
        self.start = t - x as f64 / self.px_per_ns;
    }
}

/// A "nice" tick spacing (1, 2 or 5 × a power of ten, in ns) that is at least `min_px` wide.
pub fn tick_step(px_per_ns: f64, min_px: f64) -> f64 {
    let want = min_px / px_per_ns;
    let mag = 10f64.powf(want.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= want)
        .unwrap_or(10.0 * mag)
}

/// Start times of the value changes of `s` inside `[from, to]`, for "previous/next edge".
pub fn edges(s: &Signal) -> Vec<u64> {
    let mut t = s.start_time();
    let mut out = vec![];
    for (i, (_, d)) in s.segments().enumerate() {
        if i > 0 {
            out.push(t);
        }
        t += d;
    }
    out
}

/// The edge strictly after / before `t` among all signals' edges.
pub fn neighbour_edge(model: &LogModel, t: u64, forward: bool) -> Option<u64> {
    let all = model.signals().iter().flat_map(edges);
    if forward {
        all.filter(|e| *e > t).min()
    } else {
        all.filter(|e| *e < t).max()
    }
}

pub struct WaveState {
    pub view: TimeView,
    pub follow: bool,
    pub cursor: Option<u64>,
    fitted: bool,
}

impl Default for WaveState {
    fn default() -> Self {
        WaveState {
            view: TimeView {
                start: 0.0,
                px_per_ns: 0.01,
            },
            follow: true,
            cursor: None,
            fitted: false,
        }
    }
}

struct Palette {
    line: Color32,
    high: Color32,
    unknown: Color32,
    error: Color32,
    grid: Color32,
    cursor: Color32,
    text: Color32,
    dim: Color32,
}

impl Palette {
    fn of(ui: &egui::Ui) -> Palette {
        if ui.visuals().dark_mode {
            Palette {
                line: Color32::from_rgb(120, 220, 150),
                high: Color32::from_rgb(120, 220, 150),
                unknown: Color32::from_rgb(130, 170, 255),
                error: Color32::from_rgb(255, 110, 110),
                grid: Color32::from_gray(60),
                cursor: Color32::from_rgb(255, 205, 90),
                text: Color32::from_gray(230),
                dim: Color32::from_gray(150),
            }
        } else {
            Palette {
                line: Color32::from_rgb(0, 110, 50),
                high: Color32::from_rgb(0, 110, 50),
                unknown: Color32::from_rgb(40, 80, 190),
                error: Color32::from_rgb(190, 20, 20),
                grid: Color32::from_gray(210),
                cursor: Color32::from_rgb(190, 110, 0),
                text: Color32::from_gray(20),
                dim: Color32::from_gray(100),
            }
        }
    }
}

/// How a value is drawn on a one-bit line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    High,
    Low,
    Unknown,
    Error,
}

fn level(v: &Value) -> Level {
    if v.is_error() {
        Level::Error
    } else if *v == Value::TRUE {
        Level::High
    } else if *v == Value::FALSE {
        Level::Low
    } else {
        Level::Unknown
    }
}

/// The toolbar above the diagram: zoom, fit, follow, cursor stepping.
pub fn toolbar(ui: &mut egui::Ui, model: &LogModel, st: &mut WaveState, plot_w: f32) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Zoom in").clicked() {
            st.view.zoom(1.5, plot_w / 2.0);
            st.follow = false;
        }
        if ui.button("Zoom out").clicked() {
            st.view.zoom(1.0 / 1.5, plot_w / 2.0);
            st.follow = false;
        }
        if ui.button("Fit all").clicked() {
            st.view = TimeView::fit(model.start_time() as f64, model.end_time() as f64, plot_w);
            st.follow = false;
        }
        ui.checkbox(&mut st.follow, "Follow latest");
        ui.separator();
        let cur = st.cursor;
        if ui
            .add_enabled(
                cur.is_some_and(|c| neighbour_edge(model, c, false).is_some()),
                egui::Button::new("◀ Previous edge"),
            )
            .clicked()
        {
            st.cursor = cur.and_then(|c| neighbour_edge(model, c, false));
        }
        if ui
            .add_enabled(
                cur.is_some_and(|c| neighbour_edge(model, c, true).is_some()),
                egui::Button::new("Next edge ▶"),
            )
            .clicked()
        {
            st.cursor = cur.and_then(|c| neighbour_edge(model, c, true));
        }
        match st.cursor {
            Some(c) => ui.label(format!("Cursor at {}", format_duration(c))),
            None => ui.weak("Click the diagram to place the cursor"),
        };
    });
}

/// Draw the diagram. Returns the plot width in pixels (for the toolbar's zoom buttons).
pub fn show(ui: &mut egui::Ui, model: &LogModel, st: &mut WaveState) -> f32 {
    let pal = Palette::of(ui);
    let rows = model.signal_count();
    let avail = ui.available_width();
    let plot_w = (avail - NAME_W - 8.0).max(60.0);
    if !st.fitted && model.end_time() > 0 {
        st.view = TimeView::fit(
            model.start_time() as f64,
            (model.end_time() as f64) * 1.05,
            plot_w,
        );
        st.fitted = true;
    }
    if st.follow {
        let end = model.end_time() as f64;
        let visible = plot_w as f64 / st.view.px_per_ns;
        if end > st.view.start + visible || end < st.view.start {
            st.view.start = (end - visible * 0.95).max(0.0);
        }
    }
    let height = AXIS_H + ROW_H * rows.max(1) as f32;
    let (resp, painter) = ui.allocate_painter(Vec2::new(avail, height), Sense::click_and_drag());
    let origin = resp.rect.min;
    let plot = Rect::from_min_size(
        Pos2::new(origin.x + NAME_W, origin.y),
        Vec2::new(plot_w, height),
    );

    // interaction: drag pans, ctrl+scroll / pinch zooms, scroll pans, click places the cursor
    if resp.dragged() {
        st.view.start -= resp.drag_delta().x as f64 / st.view.px_per_ns;
        st.follow = false;
    }
    if resp.hovered() {
        let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
        if zoom != 1.0 {
            let x = resp.hover_pos().map_or(plot_w / 2.0, |p| p.x - plot.min.x);
            st.view.zoom(zoom as f64, x);
            st.follow = false;
        } else if scroll.x != 0.0 || scroll.y != 0.0 {
            st.view.start -= (scroll.x + scroll.y) as f64 / st.view.px_per_ns;
            st.follow = false;
        }
    }
    if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            if p.x >= plot.min.x {
                let t = st.view.t_of(p.x - plot.min.x).max(0.0);
                st.cursor = Some(t.round() as u64);
            }
        }
    }

    let clip = painter.with_clip_rect(plot);
    draw_axis(&clip, plot, &st.view, &pal, height);
    let font = FontId::proportional(13.0);
    for (i, sig) in model.signals().iter().enumerate() {
        let top = origin.y + AXIS_H + ROW_H * i as f32;
        let row = Rect::from_min_size(Pos2::new(plot.min.x, top), Vec2::new(plot_w, ROW_H));
        painter.line_segment(
            [
                Pos2::new(origin.x, top + ROW_H),
                Pos2::new(plot.max.x, top + ROW_H),
            ],
            Stroke::new(1.0, pal.grid),
        );
        painter.text(
            Pos2::new(origin.x + 4.0, top + ROW_H / 2.0),
            Align2::LEFT_CENTER,
            sig.info.display_name(),
            font.clone(),
            pal.text,
        );
        if let Some(c) = st.cursor {
            painter.text(
                Pos2::new(plot.min.x - 6.0, top + ROW_H / 2.0),
                Align2::RIGHT_CENTER,
                sig.formatted_value_at(c),
                FontId::monospace(12.0),
                pal.cursor,
            );
        }
        draw_signal(&painter.with_clip_rect(row), row, sig, &st.view, &pal);
    }
    if rows == 0 {
        painter.text(
            plot.center(),
            Align2::CENTER_CENTER,
            "No signals recorded. Choose signals in the Logging window.",
            font,
            pal.dim,
        );
    }
    if let Some(c) = st.cursor {
        let x = plot.min.x + st.view.x_of(c as f64);
        clip.line_segment(
            [Pos2::new(x, origin.y), Pos2::new(x, origin.y + height)],
            Stroke::new(2.0, pal.cursor),
        );
    }
    plot_w
}

fn draw_axis(p: &egui::Painter, plot: Rect, view: &TimeView, pal: &Palette, height: f32) {
    let step = tick_step(view.px_per_ns, 80.0);
    let mut t = (view.start / step).floor() * step;
    let end = view.t_of(plot.width());
    while t <= end {
        let x = plot.min.x + view.x_of(t);
        p.line_segment(
            [
                Pos2::new(x, plot.min.y + AXIS_H - 4.0),
                Pos2::new(x, plot.min.y + height),
            ],
            Stroke::new(1.0, pal.grid),
        );
        if t >= 0.0 {
            p.text(
                Pos2::new(x + 3.0, plot.min.y + 2.0),
                Align2::LEFT_TOP,
                format_duration(t.round() as u64),
                FontId::proportional(11.0),
                pal.dim,
            );
        }
        t += step;
    }
}

fn draw_signal(p: &egui::Painter, row: Rect, sig: &Signal, view: &TimeView, pal: &Palette) {
    let (hi, lo, mid) = (row.min.y + 6.0, row.max.y - 6.0, row.center().y);
    let one_bit = sig.width().get() == 1;
    let mut t = sig.start_time();
    let mut prev: Option<Level> = None;
    for (v, d) in sig.segments() {
        let x0 = row.min.x + view.x_of(t as f64);
        let x1 = row.min.x + view.x_of((t + d) as f64);
        t += d;
        if x1 < row.min.x || x0 > row.max.x {
            prev = Some(level(&v));
            continue;
        }
        if one_bit {
            let lv = level(&v);
            let (y, col) = match lv {
                Level::High => (hi, pal.high),
                Level::Low => (lo, pal.line),
                Level::Unknown => (mid, pal.unknown),
                Level::Error => (mid, pal.error),
            };
            if let Some(pl) = prev {
                let py = match pl {
                    Level::High => hi,
                    Level::Low => lo,
                    _ => mid,
                };
                if py != y {
                    p.line_segment(
                        [Pos2::new(x0, py), Pos2::new(x0, y)],
                        Stroke::new(2.0, pal.line),
                    );
                }
            }
            let stroke = Stroke::new(2.0, col);
            match lv {
                Level::Unknown => p.extend(Shape::dashed_line(
                    &[Pos2::new(x0, y), Pos2::new(x1, y)],
                    stroke,
                    5.0,
                    4.0,
                )),
                _ => {
                    p.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], stroke);
                }
            }
            if matches!(lv, Level::Unknown | Level::Error) && x1 - x0 > 14.0 {
                let txt = if lv == Level::Error { "E" } else { "x" };
                p.text(
                    Pos2::new((x0 + x1) / 2.0, y - 2.0),
                    Align2::CENTER_BOTTOM,
                    txt,
                    FontId::proportional(11.0),
                    col,
                );
            }
            prev = Some(lv);
        } else {
            let col = if v.is_error() {
                pal.error
            } else if v.is_fully_defined() {
                pal.line
            } else {
                pal.unknown
            };
            let c = 4.0f32.min((x1 - x0) / 2.0);
            let pts = vec![
                Pos2::new(x0, mid),
                Pos2::new(x0 + c, hi),
                Pos2::new(x1 - c, hi),
                Pos2::new(x1, mid),
                Pos2::new(x1 - c, lo),
                Pos2::new(x0 + c, lo),
                Pos2::new(x0, mid),
            ];
            p.add(Shape::line(pts, Stroke::new(1.5, col)));
            let label = sig.info.format(&v);
            let approx_w = label.chars().count() as f32 * 7.0 + 6.0;
            if x1 - x0 > approx_w {
                p.text(
                    Pos2::new((x0 + x1) / 2.0, mid),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::monospace(12.0),
                    pal.line,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_log::{SignalInfo, SourceId};

    #[test]
    fn view_maps_time_and_zooms_around_a_point() {
        let mut v = TimeView {
            start: 100.0,
            px_per_ns: 2.0,
        };
        assert_eq!(v.x_of(150.0), 100.0);
        assert_eq!(v.t_of(100.0), 150.0);
        let t = v.t_of(40.0);
        v.zoom(2.0, 40.0);
        assert_eq!(v.px_per_ns, 4.0);
        assert!((v.t_of(40.0) - t).abs() < 1e-9);
        let f = TimeView::fit(0.0, 1000.0, 500.0);
        assert_eq!((f.start, f.px_per_ns), (0.0, 0.5));
    }

    #[test]
    fn tick_steps_are_round_numbers() {
        assert_eq!(tick_step(1.0, 80.0), 100.0);
        assert_eq!(tick_step(0.1, 80.0), 1000.0);
        assert_eq!(tick_step(10.0, 80.0), 10.0);
        assert_eq!(tick_step(0.5, 80.0), 200.0);
    }

    #[test]
    fn edges_are_where_the_value_changes() {
        let info = SignalInfo::new(SourceId(0), "a", 1);
        let mut s = Signal::new(info, Value::FALSE, 10, 0, 0);
        s.extend(Some(Value::TRUE), 5);
        s.extend(Some(Value::FALSE), 5);
        assert_eq!(edges(&s), vec![10, 15]);
    }

    #[test]
    fn levels() {
        assert_eq!(level(&Value::TRUE), Level::High);
        assert_eq!(level(&Value::FALSE), Level::Low);
        assert_eq!(level(&Value::UNKNOWN), Level::Unknown);
        assert_eq!(level(&Value::ERROR), Level::Error);
    }
}
