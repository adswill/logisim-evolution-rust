//! Canvas: drawing and mouse interaction.

//! The egui application: panels, canvas interaction, simulation control and file handling.
//! Document logic lives in `model`; this file turns input into model calls and draws.

use crate::model::{Sel, P};
use crate::paint::{self, Gfx, PartView, Theme, View};
use eframe::egui::{self, Align2, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};
use logisim_core::Value;
use logisim_format::{ComponentDef, SUBCIRCUIT_LIBRARY};
use std::collections::HashMap;

use crate::app::*;

impl App {
    // ---- canvas ------------------------------------------------------------------------------------

    pub(crate) fn canvas(&mut self, ui: &mut egui::Ui) {
        let th = Theme::new(self.mode);
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        let painter = painter.with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, th.bg);

        let mut view = self.view();
        if resp.dragged_by(PointerButton::Secondary) || resp.dragged_by(PointerButton::Middle) {
            view.origin += resp.drag_delta();
        }
        if resp.hovered() {
            let (scroll, pinch, hover) = ui.input(|i| {
                (
                    i.smooth_scroll_delta.y,
                    i.zoom_delta(),
                    i.pointer.hover_pos(),
                )
            });
            let factor = (scroll * 0.0025).exp() * pinch;
            if let (true, Some(h)) = ((factor - 1.0).abs() > 1e-4, hover) {
                let (wx, wy) = view.to_world(h);
                view.zoom = (view.zoom * factor).clamp(0.25, 6.0);
                view.origin = Vec2::new(h.x - wx * view.zoom, h.y - wy * view.zoom);
            }
        }
        self.views.insert(self.ed.current.clone(), view);

        self.cursor_world = resp.hover_pos().map(|p| view.to_world(p));
        self.hover = self.cursor_world.map(|(x, y)| (snap(x), snap(y)));
        self.hover_net = self
            .cursor_world
            .filter(|w| self.hit_part(*w).is_none())
            .and_then(|w| self.hit_wire(w))
            .and_then(|wi| self.nets.wire_net.get(wi).copied());
        let press = ui
            .input(|i| i.pointer.press_origin())
            .map(|p| view.to_world(p));

        self.draw_grid(&painter, rect, view, &th);
        self.draw_wires(&painter, view, &th);
        self.draw_parts(&painter, view, &th);

        match self.tool.clone() {
            Tool::Select => self.select_tool(&resp, &painter, view, &th, press, ui),
            Tool::Poke => {
                if resp.clicked_by(PointerButton::Primary) {
                    if let Some(i) = self.cursor_world.and_then(|w| self.hit_part(w)) {
                        self.poke(i);
                    }
                }
            }
            Tool::Wire => self.wire_tool(&resp, &painter, view, &th, press),
            Tool::Place(lib, part) => self.place_tool(&resp, &painter, view, &th, &lib, &part),
        }
        if resp.clicked_by(PointerButton::Secondary) {
            self.tool = Tool::Select;
            self.wire_chain = None;
        }
        self.draw_net_tooltip(&painter, &th, resp.hover_pos());
        if let Some((x, y)) = self.hover {
            if self.infos.iter().any(|i| i.abs.contains(&(x, y))) {
                painter.circle_stroke(
                    view.to_screen(x as f32, y as f32),
                    7.0,
                    Stroke::new(1.5, th.select),
                );
            }
        }
    }

    pub(crate) fn select_tool(
        &mut self,
        resp: &egui::Response,
        painter: &egui::Painter,
        view: View,
        th: &Theme,
        press: Option<(f32, f32)>,
        ui: &egui::Ui,
    ) {
        let shift = ui.input(|i| i.modifiers.shift);
        if resp.drag_started_by(PointerButton::Primary) {
            if let Some(w) = press {
                match self.hit_any(w) {
                    Some(h) => {
                        if !self.ed.selection.contains(&h) {
                            self.ed.selection = vec![h];
                        }
                        self.drag = Drag::Move { origin: w };
                    }
                    None => {
                        if !shift {
                            self.ed.selection.clear();
                        }
                        self.drag = Drag::Box { origin: w };
                    }
                }
            }
        }
        if let Drag::Box { origin } = &self.drag {
            if let Some(c) = self.cursor_world {
                let r = Rect::from_two_pos(
                    view.to_screen(origin.0, origin.1),
                    view.to_screen(c.0, c.1),
                );
                painter.rect_stroke(
                    r,
                    2.0,
                    Stroke::new(1.0, th.select),
                    egui::StrokeKind::Middle,
                );
                painter.rect_filled(r, 2.0, th.select.gamma_multiply(0.12));
            }
        }
        if resp.drag_stopped_by(PointerButton::Primary) {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::Move { .. } => {
                    let (dx, dy) = {
                        // the offset was computed from the last cursor position
                        let c = self.cursor_world;
                        let o = press.unwrap_or((0.0, 0.0));
                        c.map_or((0, 0), |c| (snap(c.0 - o.0), snap(c.1 - o.1)))
                    };
                    self.ed.move_selection(dx, dy);
                }
                Drag::Box { origin } => {
                    if let Some(c) = self.cursor_world {
                        self.select_in(
                            Rect::from_two_pos(Pos2::new(origin.0, origin.1), Pos2::new(c.0, c.1)),
                            shift,
                        );
                    }
                }
                _ => {}
            }
        }
        if resp.clicked_by(PointerButton::Primary) {
            match self.cursor_world.and_then(|w| self.hit_any(w)) {
                Some(h) if shift => {
                    if let Some(pos) = self.ed.selection.iter().position(|s| *s == h) {
                        self.ed.selection.remove(pos);
                    } else {
                        self.ed.selection.push(h);
                    }
                }
                Some(h) => {
                    // clicking an input Pin while simulating flips it (no tool switch needed)
                    if let Sel::Comp(i) = h {
                        if self.sim_on && self.is_input_pin(i) {
                            self.poke(i);
                        }
                    }
                    self.ed.selection = vec![h];
                }
                None => self.ed.selection.clear(),
            }
        }
        if resp.double_clicked() {
            if let Some(i) = self.cursor_world.and_then(|w| self.hit_part(w)) {
                let def = &self.ed.circuit().components[i];
                if def.library == SUBCIRCUIT_LIBRARY {
                    let name = def.part.clone();
                    self.ed.open_circuit(&name);
                    self.status = format!("Opened circuit '{name}'");
                }
            }
        }
    }

    pub(crate) fn wire_tool(
        &mut self,
        resp: &egui::Response,
        painter: &egui::Painter,
        view: View,
        th: &Theme,
        press: Option<(f32, f32)>,
    ) {
        if resp.drag_started_by(PointerButton::Primary) {
            if let Some(w) = press {
                self.drag = Drag::Wire {
                    start: (snap(w.0), snap(w.1)),
                };
                self.wire_chain = None;
            }
        }
        let start = match self.drag {
            Drag::Wire { start } => Some(start),
            _ => self.wire_chain,
        };
        if let (Some(s), Some(h)) = (start, self.hover) {
            let corner = (h.0, s.1);
            let pts = [s, corner, h].map(|p| view.to_screen(p.0 as f32, p.1 as f32));
            painter.line_segment([pts[0], pts[1]], Stroke::new(2.0, th.select));
            painter.line_segment([pts[1], pts[2]], Stroke::new(2.0, th.select));
        }
        if resp.drag_stopped_by(PointerButton::Primary) {
            if let (Drag::Wire { start }, Some(h)) =
                (std::mem::replace(&mut self.drag, Drag::None), self.hover)
            {
                self.ed.add_wire_path(start, h);
            }
        }
        if resp.double_clicked() {
            self.wire_chain = None;
        } else if resp.clicked_by(PointerButton::Primary) {
            if let Some(h) = self.hover {
                if let Some(s) = self.wire_chain {
                    self.ed.add_wire_path(s, h);
                }
                self.wire_chain = Some(h);
            }
        }
    }

    pub(crate) fn place_tool(
        &mut self,
        resp: &egui::Response,
        painter: &egui::Painter,
        view: View,
        th: &Theme,
        lib: &str,
        part: &str,
    ) {
        let Some((x, y)) = self.hover else { return };
        let made = if lib == SUBCIRCUIT_LIBRARY {
            self.ed.subcircuit_def(part, (x, y))
        } else {
            ComponentDef::new(lib, part, x, y).map_err(|e| e.to_string())
        };
        let mut def = match made {
            Ok(d) => d,
            Err(e) => {
                self.status = e;
                self.tool = Tool::Select;
                return;
            }
        };
        def.facing = self.place_facing;
        let info = make_info(&self.ed.project, &def);
        let vals: Vec<Option<Value>> = vec![];
        let v = PartView {
            def: &def,
            name: &info.name,
            ports: &info.ports,
            port_vals: &vals,
            selected: false,
            port_names: &info.port_names,
        };
        let g = Gfx {
            painter,
            view,
            theme: th,
            at: def.at,
            facing: def.facing,
            alpha: 0.55,
        };
        paint::paint_part(&g, &v);
        if resp.clicked_by(PointerButton::Primary) {
            self.ed.place(def);
        }
    }

    pub(crate) fn draw_grid(&self, painter: &egui::Painter, rect: Rect, view: View, th: &Theme) {
        if !self.show_grid {
            return;
        }
        let step = GRID as f32 * view.zoom;
        if step >= 7.0 {
            let (x0, y0) = view.to_world(rect.min);
            let (x1, y1) = view.to_world(rect.max);
            let (gx0, gy0) = (
                (x0 / GRID as f32).floor() as i32,
                (y0 / GRID as f32).floor() as i32,
            );
            let (gx1, gy1) = (
                (x1 / GRID as f32).ceil() as i32,
                (y1 / GRID as f32).ceil() as i32,
            );
            for gx in gx0..=gx1 {
                for gy in gy0..=gy1 {
                    let major = gx % 5 == 0 && gy % 5 == 0;
                    let c = if major { th.grid_major } else { th.grid };
                    painter.circle_filled(
                        view.to_screen((gx * GRID) as f32, (gy * GRID) as f32),
                        if major { 1.6 } else { 1.0 },
                        c,
                    );
                }
            }
        } else {
            let big = 50.0 * view.zoom;
            let mut x = rect.min.x + (view.origin.x - rect.min.x).rem_euclid(big);
            while x < rect.max.x {
                painter.vline(x, rect.y_range(), Stroke::new(1.0, th.grid));
                x += big;
            }
            let mut y = rect.min.y + (view.origin.y - rect.min.y).rem_euclid(big);
            while y < rect.max.y {
                painter.hline(rect.x_range(), y, Stroke::new(1.0, th.grid));
                y += big;
            }
        }
    }

    /// Value of the net a wire belongs to (None when the simulation is off).
    pub(crate) fn wire_value(&self, i: usize) -> Option<Value> {
        let w = self.ed.circuit().wires[i];
        self.sim.as_ref().and_then(|s| s.value_at(loc(w.a)))
    }

    pub(crate) fn draw_wires(&self, painter: &egui::Painter, view: View, th: &Theme) {
        let c = self.ed.circuit();
        let (dx, dy) = self.move_offset();
        let screen = |p: P, o: (i32, i32)| view.to_screen((p.0 + o.0) as f32, (p.1 + o.1) as f32);
        // halo behind every wire of the hovered net
        if let Some(n) = self.hover_net.and_then(|n| self.nets.nets.get(n)) {
            for &wi in &n.wires {
                let w = c.wires[wi];
                painter.line_segment(
                    [screen(w.a, (0, 0)), screen(w.b, (0, 0))],
                    Stroke::new((9.0 * view.zoom).max(6.0), th.select.gamma_multiply(0.28)),
                );
            }
        }
        for (i, w) in c.wires.iter().enumerate() {
            let selected = self.ed.selection.contains(&Sel::Wire(i));
            let val = self.wire_value(i);
            let off = if selected { (dx, dy) } else { (0, 0) };
            let (a, b) = (screen(w.a, off), screen(w.b, off));
            let hue = self
                .nets
                .wire_net
                .get(i)
                .and_then(|n| self.nets.nets.get(*n))
                .map(|n| n.hue);
            let (color, thick, dashed) = if selected {
                (th.select, 2.5, false)
            } else if let (true, Some(h)) = (self.colour_nets, hue) {
                let base = th.net_color(h, th.dark);
                match val {
                    None => (base, 2.5, false),
                    Some(v) if v.is_error() => (th.v_err, 3.5, false),
                    Some(v) if v.width().get() > 1 && v.is_unknown() => (th.dim, 4.0, true),
                    Some(v) if v.width().get() > 1 => (base, 4.5, false),
                    Some(v) if v == Value::TRUE => (base, 3.4, false),
                    Some(v) if v == Value::FALSE => (base.gamma_multiply(0.45), 2.0, false),
                    Some(_) => (th.dim, 2.2, true),
                }
            } else {
                let thick = if val.is_some_and(|v| v.width().get() > 1) {
                    3.5
                } else {
                    2.0
                };
                (th.value_color(val), thick, false)
            };
            let stroke = Stroke::new((thick * view.zoom).max(1.5), color);
            if dashed {
                painter.extend(egui::Shape::dashed_line(
                    &[a, b],
                    stroke,
                    7.0 * view.zoom.max(0.6),
                    5.0 * view.zoom.max(0.6),
                ));
            } else {
                painter.line_segment([a, b], stroke);
            }
        }
        // one label per net: bus width, or a warning for errors (on its longest wire)
        if self.colour_nets {
            for n in &self.nets.nets {
                let Some(&wi) = n.wires.iter().max_by_key(|&&i| {
                    (c.wires[i].a.0 - c.wires[i].b.0).abs()
                        + (c.wires[i].a.1 - c.wires[i].b.1).abs()
                }) else {
                    continue;
                };
                let Some(v) = self.wire_value(wi) else {
                    continue;
                };
                let w = c.wires[wi];
                let mid = view
                    .to_screen((w.a.0 + w.b.0) as f32 / 2.0, (w.a.1 + w.b.1) as f32 / 2.0)
                    + Vec2::new(0.0, -9.0 * view.zoom.max(0.7));
                let font = egui::FontId::proportional((10.0 * view.zoom).clamp(8.0, 16.0));
                if v.is_error() {
                    painter.text(mid, Align2::CENTER_BOTTOM, "! error", font, th.v_err);
                } else if v.width().get() > 1 {
                    painter.text(
                        mid,
                        Align2::CENTER_BOTTOM,
                        format!("{} bits", v.width().get()),
                        font,
                        th.dim,
                    );
                }
            }
        }
        // junction dots: three or more connections meet (a T counts the through wire twice)
        let mut points: HashMap<P, usize> = HashMap::new();
        for w in &c.wires {
            *points.entry(w.a).or_default() += 1;
            *points.entry(w.b).or_default() += 1;
        }
        for info in &self.infos {
            for p in &info.abs {
                *points.entry(*p).or_default() += 1;
            }
        }
        for (&p, n) in &points {
            let through = c
                .wires
                .iter()
                .filter(|w| {
                    let (lo, hi) = (
                        (w.a.0.min(w.b.0), w.a.1.min(w.b.1)),
                        (w.a.0.max(w.b.0), w.a.1.max(w.b.1)),
                    );
                    p != w.a && p != w.b && p.0 >= lo.0 && p.0 <= hi.0 && p.1 >= lo.1 && p.1 <= hi.1
                })
                .count();
            if n + 2 * through >= 3 {
                let val = self.sim.as_ref().and_then(|s| s.value_at(loc(p)));
                let color = match (
                    self.colour_nets,
                    self.point_net.get(&p).and_then(|i| self.nets.nets.get(*i)),
                ) {
                    (true, Some(net)) => th.net_color(net.hue, th.dark),
                    _ => th.value_color(val),
                };
                painter.circle_filled(
                    view.to_screen(p.0 as f32, p.1 as f32),
                    (3.5 * view.zoom).max(2.5),
                    color,
                );
            }
        }
    }

    /// Small readout of the hovered net's value in decimal, hex and binary.
    pub(crate) fn draw_net_tooltip(
        &self,
        painter: &egui::Painter,
        th: &Theme,
        cursor: Option<Pos2>,
    ) {
        let (Some(n), Some(pos)) = (self.hover_net.and_then(|n| self.nets.nets.get(n)), cursor)
        else {
            return;
        };
        let mut lines = vec![];
        match n.wires.first().and_then(|&w| self.wire_value(w)) {
            None => lines.push("simulation is off".to_string()),
            Some(v) => {
                let w = v.width().get() as usize;
                lines.push(format!("{w}-bit net"));
                match v.to_u64() {
                    Some(x) if w == 1 => lines.push(format!("value {x}")),
                    Some(x) => {
                        lines.push(format!("dec  {x}"));
                        lines.push(format!("hex  0x{x:X}"));
                        lines.push(format!("bin  0b{x:0w$b}"));
                    }
                    None => lines.push(format!("bits {}", v.to_binary_string())),
                }
            }
        }
        let font = egui::FontId::monospace(12.0);
        let text = lines.join("\n");
        let galley = painter.layout_no_wrap(text, font, th.text);
        let rect = Rect::from_min_size(
            pos + Vec2::new(14.0, 14.0),
            galley.size() + Vec2::splat(12.0),
        );
        painter.rect_filled(rect, 6.0, th.fill);
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, th.select),
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.min + Vec2::splat(6.0), galley, th.text);
    }

    pub(crate) fn draw_parts(&self, painter: &egui::Painter, view: View, th: &Theme) {
        let (dx, dy) = self.move_offset();
        for (i, def) in self.ed.circuit().components.iter().enumerate() {
            let selected = self.ed.selection.contains(&Sel::Comp(i));
            let vals = self.port_vals(i);
            let v = self.part_view(i, selected, &vals);
            let at = if selected {
                (def.at.0 + dx, def.at.1 + dy)
            } else {
                def.at
            };
            let g = Gfx {
                painter,
                view,
                theme: th,
                at,
                facing: def.facing,
                alpha: 1.0,
            };
            paint::paint_part(&g, &v);
            if let Some(err) = &self.infos[i].error {
                let pos = view.to_screen(at.0 as f32, at.1 as f32 + 14.0);
                painter.text(
                    pos,
                    Align2::CENTER_TOP,
                    format!("Error: {err}"),
                    egui::FontId::proportional(10.0),
                    th.v_err,
                );
            }
        }
    }
}
