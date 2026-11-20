//! Canvas extras: problem badges, port hover, snap feedback, context menu, minimap, welcome card.

use crate::actions::Action;
use crate::app::*;
use crate::diagnostics::{self, PartPorts, Problem};
use crate::model::P;
use crate::paint::{self, Theme, View};
use eframe::egui::{self, Align2, Pos2, Rect, Sense, Stroke, Vec2};
use logisim_core::PortKind;
use logisim_format::SUBCIRCUIT_LIBRARY;

/// Screen-space radius within which a port or badge reacts to the pointer (generous on purpose).
const HOVER_PX: f32 = 11.0;

impl App {
    /// Whether the part currently drives X or E on an output (live simulation only).
    fn output_unknown(&self, comp: usize) -> bool {
        let Some(sim) = &self.sim else { return false };
        let info = &self.infos[comp];
        info.ports.iter().zip(&info.abs).any(|(p, a)| {
            p.kind == PortKind::Output
                && sim
                    .value_at(logisim_core::Location::new(a.0, a.1))
                    .is_some_and(|v| v.is_error() || v.is_unknown())
        })
    }

    /// Per-part "output is unknown" flags; when they change, problems are recomputed.
    pub(crate) fn unknown_signature(&self) -> Vec<bool> {
        (0..self.infos.len())
            .map(|i| self.output_unknown(i))
            .collect()
    }

    /// Recompute the problems of the open circuit (one entry per affected part).
    pub(crate) fn refresh_problems(&mut self) {
        let sig = self.unknown_signature();
        let parts: Vec<PartPorts> = self
            .infos
            .iter()
            .map(|i| PartPorts {
                name: &i.name,
                ports: &i.ports,
                abs: &i.abs,
            })
            .collect();
        self.problems = diagnostics::diagnose(&parts, &self.nets, &|i| sig[i]);
        self.unknown_sig = sig;
    }

    /// Small tooltip box near the pointer.
    pub(crate) fn draw_tip(painter: &egui::Painter, th: &Theme, at: Pos2, lines: &[String]) {
        let font = egui::FontId::proportional(13.0);
        let galley = painter.layout(lines.join("\n"), font, th.text, 360.0);
        let rect = Rect::from_min_size(
            at + Vec2::new(14.0, 14.0),
            galley.size() + Vec2::splat(14.0),
        );
        painter.rect_filled(rect, 6.0, th.fill);
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.2, th.select),
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.min + Vec2::splat(7.0), galley, th.text);
    }

    /// One small warning triangle per part with problems, just outside its top-right corner
    /// (never over ports or labels); returns the problem under the pointer.
    pub(crate) fn draw_badges(
        &self,
        painter: &egui::Painter,
        view: View,
        th: &Theme,
        cursor: Option<Pos2>,
    ) -> Option<Problem> {
        let mut hovered = None;
        let amber = if th.dark {
            egui::Color32::from_rgb(255, 196, 40)
        } else {
            egui::Color32::from_rgb(196, 120, 0)
        };
        let r = (4.5 * view.zoom).clamp(5.0, 8.0);
        for p in &self.problems {
            let v = self.part_view(p.comp, false, &[]);
            let b = paint::world_bounds(v.def, &v);
            let c = view.to_screen(b.max.x, b.min.y) + Vec2::new(r + 3.0, -(r + 3.0));
            let tri = vec![
                c + Vec2::new(0.0, -r),
                c + Vec2::new(r, r * 0.8),
                c + Vec2::new(-r, r * 0.8),
            ];
            painter.add(egui::Shape::convex_polygon(
                tri,
                amber,
                Stroke::new(1.0, th.bg),
            ));
            painter.text(
                c + Vec2::new(0.0, r * 0.25),
                Align2::CENTER_CENTER,
                "!",
                egui::FontId::proportional(r * 1.15),
                egui::Color32::BLACK,
            );
            if cursor.is_some_and(|m| m.distance(c) <= HOVER_PX) {
                hovered = Some(p.clone());
            }
        }
        hovered
    }

    /// Nearest port within hover distance of the pointer: (component index, port index).
    fn port_under(&self, view: View, cursor: Pos2) -> Option<(usize, usize)> {
        let mut best: Option<(f32, usize, usize)> = None;
        for (ci, info) in self.infos.iter().enumerate() {
            for (pi, a) in info.abs.iter().enumerate() {
                let d = view.to_screen(a.0 as f32, a.1 as f32).distance(cursor);
                if d <= HOVER_PX && best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, ci, pi));
                }
            }
        }
        best.map(|(_, c, p)| (c, p))
    }

    /// Hover feedback: port highlight + description, problem badge explanation, snap marker.
    pub(crate) fn hover_feedback(
        &self,
        painter: &egui::Painter,
        view: View,
        th: &Theme,
        cursor: Option<Pos2>,
    ) {
        let Some(m) = cursor else { return };
        if let Some(p) = self.draw_badges(painter, view, th, Some(m)) {
            Self::draw_tip(painter, th, m, &p.tooltip(&self.infos[p.comp].name));
            return;
        }
        if let Some((ci, pi)) = self.port_under(view, m) {
            let info = &self.infos[ci];
            let a = info.abs[pi];
            painter.circle_stroke(
                view.to_screen(a.0 as f32, a.1 as f32),
                9.0,
                Stroke::new(2.0, th.select),
            );
            let port = &info.ports[pi];
            let dir = match port.kind {
                PortKind::Input => "input",
                PortKind::Output => "output",
                PortKind::InOut => "bidirectional",
            };
            let w = port.width.get();
            let width = if w == 0 {
                "any width".to_string()
            } else {
                format!("{w} bit{}", if w == 1 { "" } else { "s" })
            };
            let name = info
                .port_names
                .get(pi)
                .map(|n| format!("{n}: "))
                .unwrap_or_default();
            let mut lines = vec![format!("{}{dir}, {width}", name), info.name.clone()];
            if let Some(v) = self
                .sim
                .as_ref()
                .and_then(|s| s.value_at(logisim_core::Location::new(a.0, a.1)))
            {
                lines.push(format!("value {}", paint::value_text(&v)));
            }
            Self::draw_tip(painter, th, m, &lines);
        }
    }

    /// Crosshair at the snapped point while placing or wiring; ring + "connects" over a port or wire end.
    pub(crate) fn draw_snap(&self, painter: &egui::Painter, view: View, th: &Theme) {
        if !matches!(self.tool, Tool::Wire | Tool::Place(..)) {
            return;
        }
        let Some((x, y)) = self.hover else { return };
        let c = view.to_screen(x as f32, y as f32);
        let on_port = self.infos.iter().any(|i| i.abs.contains(&(x, y)));
        let on_wire_end = self
            .ed
            .circuit()
            .wires
            .iter()
            .any(|w| w.a == (x, y) || w.b == (x, y));
        let col = if on_port || on_wire_end {
            th.v_true
        } else {
            th.select
        };
        painter.line_segment(
            [c - Vec2::new(6.0, 0.0), c + Vec2::new(6.0, 0.0)],
            Stroke::new(1.5, col),
        );
        painter.line_segment(
            [c - Vec2::new(0.0, 6.0), c + Vec2::new(0.0, 6.0)],
            Stroke::new(1.5, col),
        );
        if on_port || on_wire_end {
            painter.circle_stroke(c, 8.0, Stroke::new(2.0, col));
            painter.text(
                c + Vec2::new(12.0, -12.0),
                Align2::LEFT_BOTTOM,
                "connects here",
                egui::FontId::proportional(11.0),
                col,
            );
        }
    }

    /// Right-click menu for the item under the pointer (selected by `canvas`).
    pub(crate) fn canvas_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let has_sel = !self.ed.selection.is_empty();
        let part = match self.ed.selection[..] {
            [crate::model::Sel::Comp(i)] => Some(i),
            _ => None,
        };
        if let Some(i) = part {
            let def = self.ed.circuit().components[i].clone();
            ui.strong(&self.infos[i].name);
            if def.library == SUBCIRCUIT_LIBRARY && ui.button("Open this circuit").clicked() {
                self.ed.open_circuit(&def.part);
                ui.close();
            }
            if self.sim_on && self.is_input_pin(i) && ui.button("Flip pin value").clicked() {
                self.poke(i);
                ui.close();
            }
            if ui.button("Edit attributes").clicked() {
                ctx.memory_mut(|m| m.request_focus(self.attr_id));
                ui.close();
            }
            ui.separator();
        }
        for a in [Action::Rotate, Action::Duplicate, Action::Cut, Action::Copy] {
            let mut b = egui::Button::new(a.label());
            if let Some(sc) = a.shortcut() {
                b = b.shortcut_text(sc);
            }
            if ui.add_enabled(has_sel, b).clicked() {
                self.run_action(a, &ctx);
                ui.close();
            }
        }
        if ui
            .add_enabled(
                self.ed.has_clipboard(),
                egui::Button::new("Paste here")
                    .shortcut_text(Action::Paste.shortcut().unwrap_or_default()),
            )
            .clicked()
        {
            self.paste_clip();
            ui.close();
        }
        ui.separator();
        let del = egui::Button::new(Action::Delete.label()).shortcut_text("Delete");
        if ui.add_enabled(has_sel, del).clicked() {
            self.run_action(Action::Delete, &ctx);
            ui.close();
        }
        if !has_sel {
            ui.separator();
            for a in [Action::SelectAll, Action::FitView] {
                if ui.button(a.label()).clicked() {
                    self.run_action(a, &ctx);
                    ui.close();
                }
            }
        }
    }

    /// Overview of the whole circuit in a corner; drag or click to move the view.
    pub(crate) fn minimap(&mut self, ui: &mut egui::Ui, canvas: Rect, th: &Theme) {
        if !(self.prefs.show_minimap || self.dev_minimap) {
            return;
        }
        let Some(content) = self.content_bounds() else {
            return;
        };
        let mut view = self.views.get(&self.ed.current).copied().unwrap_or(View {
            origin: canvas.min.to_vec2(),
            zoom: 1.0,
        });
        let (w0, w1) = (view.to_world(canvas.min), view.to_world(canvas.max));
        let visible = Rect::from_min_max(Pos2::new(w0.0, w0.1), Pos2::new(w1.0, w1.1));
        let all = content.union(visible).expand(20.0);
        let size = Vec2::new(190.0, 124.0);
        let map = Rect::from_min_size(canvas.max - size - Vec2::splat(12.0), size);
        let s = (size.x / all.width()).min(size.y / all.height());
        let to_map = |x: f32, y: f32| {
            map.min
                + Vec2::new((x - all.min.x) * s, (y - all.min.y) * s)
                + (size - all.size() * s) / 2.0
        };
        let painter = ui.painter_at(map.expand(2.0));
        painter.rect_filled(map, 6.0, th.fill.gamma_multiply(0.92));
        painter.rect_stroke(map, 6.0, Stroke::new(1.0, th.dim), egui::StrokeKind::Inside);
        for w in &self.ed.circuit().wires {
            painter.line_segment(
                [
                    to_map(w.a.0 as f32, w.a.1 as f32),
                    to_map(w.b.0 as f32, w.b.1 as f32),
                ],
                Stroke::new(1.0, th.stroke.gamma_multiply(0.6)),
            );
        }
        for i in 0..self.infos.len() {
            let v = self.part_view(i, false, &[]);
            let b = paint::world_bounds(v.def, &v);
            painter.rect_filled(
                Rect::from_two_pos(to_map(b.min.x, b.min.y), to_map(b.max.x, b.max.y)),
                1.0,
                th.dim,
            );
        }
        let vr = Rect::from_two_pos(
            to_map(visible.min.x, visible.min.y),
            to_map(visible.max.x, visible.max.y),
        )
        .intersect(map);
        painter.rect_stroke(
            vr,
            2.0,
            Stroke::new(1.6, th.select),
            egui::StrokeKind::Inside,
        );
        let r = ui.interact(map, egui::Id::unique("minimap"), Sense::click_and_drag());
        r.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Group,
                true,
                "Minimap: click or drag to move the view",
            )
        });
        if let (true, Some(p)) = (r.clicked() || r.dragged(), r.interact_pointer_pos()) {
            let off = (p - map.min) - (size - all.size() * s) / 2.0;
            let (wx, wy) = (all.min.x + off.x / s, all.min.y + off.y / s);
            view.origin = canvas.center().to_vec2() - Vec2::new(wx, wy) * view.zoom;
            self.views.insert(self.ed.current.clone(), view);
        }
    }

    /// Friendly empty state: how to start, recent projects and an example.
    pub(crate) fn welcome(&mut self, ui: &mut egui::Ui, canvas: Rect) {
        let empty = self.ed.circuit().components.is_empty()
            && self.ed.circuit().wires.is_empty()
            && self.ed.path.is_none()
            && self.ed.project.circuits.len() == 1
            && !self.ed.can_undo();
        if !empty {
            return;
        }
        let ctx = ui.ctx().clone();
        egui::Area::new(egui::Id::unique("welcome")).pivot(Align2::CENTER_CENTER).fixed_pos(canvas.center()).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(20.0).show(ui, |ui| {
                ui.set_max_width(420.0);
                ui.heading("Start a circuit");
                ui.label("Pick a part in the list on the left and click here to place it, then draw wires between the little dots (ports).");
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Open project…").clicked() {
                        self.run_action(Action::Open, &ctx);
                    }
                    if ui.button("Try the half-adder example").clicked() {
                        self.run_action(Action::OpenExample, &ctx);
                    }
                    if ui.button("Command palette").clicked() {
                        self.run_action(Action::CommandPalette, &ctx);
                    }
                });
                let recent: Vec<String> = self.prefs.recent_files.iter().take(5).cloned().collect();
                if !recent.is_empty() {
                    ui.add_space(8.0);
                    ui.strong("Recent projects");
                    for f in recent {
                        let name = std::path::Path::new(&f).file_name().map_or(f.clone(), |n| n.to_string_lossy().into());
                        if ui.link(name).on_hover_text(&f).clicked() {
                            self.open_path(std::path::PathBuf::from(f));
                        }
                    }
                }
                ui.add_space(8.0);
                ui.weak(format!("Tips: {}K opens the command palette · {}/ lists every shortcut · right-click for a menu", crate::panels::mod_key(), crate::panels::mod_key()));
            });
        });
    }
}

/// Snap helper kept next to its users.
#[allow(dead_code)]
pub(crate) fn same_point(a: P, b: P) -> bool {
    a == b
}
