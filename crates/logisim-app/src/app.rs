//! The egui application: panels, canvas interaction, simulation control and file handling.
//! Document logic lives in `model`; this file turns input into model calls and draws.

use crate::examples;
use crate::model::{Editor, Sel, P};
use crate::numfmt::{self, Base};
use crate::paint::{self, Gfx, PartView, Theme, ThemeMode, View};
use crate::sim::{wire_points, Sim};
use eframe::egui::{self, Align2, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};
use logisim_components::all_libraries;
use logisim_core::{Direction, Location, OptVal, OptionKind, OptionSpec, Port, PortKind, Value};
use logisim_format::{ComponentDef, ProjectFile, SUBCIRCUIT_LIBRARY};
use std::collections::HashMap;
use std::path::PathBuf;

const GRID: i32 = 10;

#[derive(Clone, PartialEq, Debug)]
enum Tool {
    Select,
    Poke,
    Wire,
    /// Place a part: (library, part); library `SUBCIRCUIT_LIBRARY` places a subcircuit.
    Place(String, String),
}

enum Drag {
    None,
    Move { origin: (f32, f32) },
    Box { origin: (f32, f32) },
    Wire { start: P },
}

/// Cached geometry of one placed part.
struct Info {
    name: String,
    ports: Vec<Port>,
    port_names: Vec<String>,
    abs: Vec<P>,
    error: Option<String>,
}

fn snap(v: f32) -> i32 {
    ((v / GRID as f32).round() as i32) * GRID
}

fn loc(p: P) -> Location {
    Location::new(p.0, p.1)
}

fn make_info(project: &ProjectFile, def: &ComponentDef) -> Info {
    let (name, ports, port_names, error) = if def.library == SUBCIRCUIT_LIBRARY {
        match project.circuit(&def.part) {
            Some(c) => {
                let ports = c
                    .interface
                    .iter()
                    .map(|i| {
                        let kind = if i.input {
                            PortKind::Input
                        } else {
                            PortKind::Output
                        };
                        Port::new(
                            i.offset.0,
                            i.offset.1,
                            kind,
                            logisim_core::BitWidth::new(i.width),
                        )
                    })
                    .collect();
                (
                    def.part.clone(),
                    ports,
                    c.interface.iter().map(|i| i.name.clone()).collect(),
                    None,
                )
            }
            None => (
                def.part.clone(),
                vec![],
                vec![],
                Some(format!("missing circuit '{}'", def.part)),
            ),
        }
    } else {
        match def.build() {
            Ok(c) => (c.name().to_string(), c.ports(), vec![], None),
            Err(e) => (def.part.clone(), vec![], vec![], Some(e.to_string())),
        }
    };
    let abs = ports
        .iter()
        .map(|p| {
            let o = def.facing.rotate(p.offset);
            (def.at.0 + o.x, def.at.1 + o.y)
        })
        .collect();
    Info {
        name,
        ports,
        port_names,
        abs,
        error,
    }
}

pub struct App {
    ed: Editor,
    mode: ThemeMode,
    views: HashMap<String, View>,
    tool: Tool,
    place_facing: Direction,
    drag: Drag,
    wire_chain: Option<P>,
    cursor_world: Option<(f32, f32)>,
    hover: Option<P>,
    infos: Vec<Info>,
    infos_key: (u64, String),
    sim: Option<Sim>,
    sim_key: (u64, String),
    sim_on: bool,
    running: bool,
    hz: f32,
    accum: f32,
    base: Base,
    attr_buf: HashMap<String, String>,
    attr_for: Option<(String, usize, String)>,
    attr_err: HashMap<String, String>,
    new_circuit: String,
    status: String,
    show_grid: bool,
    title_sent: String,
}

impl App {
    pub fn new(ctx: &egui::Context, file: Option<PathBuf>, example: bool) -> Self {
        let mut app = App {
            ed: Editor::new(),
            mode: ThemeMode::Dark,
            views: HashMap::new(),
            tool: Tool::Select,
            place_facing: Direction::East,
            drag: Drag::None,
            wire_chain: None,
            cursor_world: None,
            hover: None,
            infos: vec![],
            infos_key: (u64::MAX, String::new()),
            sim: None,
            sim_key: (u64::MAX, String::new()),
            sim_on: true,
            running: false,
            hz: 2.0,
            accum: 0.0,
            base: Base::Dec,
            attr_buf: HashMap::new(),
            attr_for: None,
            attr_err: HashMap::new(),
            new_circuit: String::new(),
            status: "Pick a part on the left, then click on the canvas to place it.".into(),
            show_grid: true,
            title_sent: String::new(),
        };
        if example {
            app.load_example();
        } else if let Some(p) = file {
            app.open_path(p);
        }
        app.apply_theme(ctx);
        app
    }

    fn load_example(&mut self) {
        if self.confirm_discard() {
            self.ed = examples::half_adder();
            self.tool = Tool::Poke;
            self.status =
                "Half adder: use the Poke tool to flip inputs A and B and watch Sum and Carry."
                    .into();
        }
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let mut v = match self.mode {
            ThemeMode::Dark => egui::Visuals::dark(),
            ThemeMode::Light => egui::Visuals::light(),
        };
        let r = egui::CornerRadius::same(6);
        v.widgets.noninteractive.corner_radius = r;
        v.widgets.inactive.corner_radius = r;
        v.widgets.hovered.corner_radius = r;
        v.widgets.active.corner_radius = r;
        ctx.set_visuals(v);
    }

    fn view(&self) -> View {
        self.views.get(&self.ed.current).copied().unwrap_or(View {
            origin: Vec2::new(300.0, 120.0),
            zoom: 1.0,
        })
    }

    // ---- files -----------------------------------------------------------------------------------

    fn confirm_discard(&self) -> bool {
        !self.ed.dirty()
            || rfd::MessageDialog::new()
                .set_title("Unsaved changes")
                .set_description("Discard the changes to this project?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes
    }

    fn new_project(&mut self) {
        if self.confirm_discard() {
            self.ed = Editor::new();
            self.status = "New project".into();
        }
    }

    fn open_dialog(&mut self) {
        if !self.confirm_discard() {
            return;
        }
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("rcirc project", &["rcirc"])
            .pick_file()
        {
            self.open_path(p);
        }
    }

    fn open_path(&mut self, p: PathBuf) {
        let parsed = std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|t| ProjectFile::parse(&t).map_err(|e| e.to_string()));
        match parsed {
            Ok(project) if project.circuit(&project.main).is_none() => {
                self.status = format!(
                    "{}: main circuit '{}' is missing",
                    p.display(),
                    project.main
                );
            }
            Ok(project) => {
                self.status = format!("Opened {}", p.display());
                self.ed = Editor::from_project(project, Some(p));
            }
            Err(e) => self.status = format!("Could not open {}: {e}", p.display()),
        }
    }

    fn save(&mut self, as_new: bool) {
        let path = match (&self.ed.path, as_new) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new()
                .add_filter("rcirc project", &["rcirc"])
                .set_file_name(format!("{}.rcirc", self.ed.project.metadata.name))
                .save_file(),
        };
        let Some(mut path) = path else { return };
        if path.extension().is_none() {
            path.set_extension("rcirc");
        }
        let written = self
            .ed
            .project
            .to_ron()
            .map_err(|e| e.to_string())
            .and_then(|t| std::fs::write(&path, t).map_err(|e| e.to_string()));
        match written {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                self.ed.path = Some(path);
                self.ed.mark_saved();
            }
            Err(e) => self.status = format!("Could not save: {e}"),
        }
    }

    // ---- caches and simulation --------------------------------------------------------------------

    fn refresh_caches(&mut self) {
        let key = (self.ed.version, self.ed.current.clone());
        if self.infos_key != key {
            self.infos = self
                .ed
                .circuit()
                .components
                .iter()
                .map(|d| make_info(&self.ed.project, d))
                .collect();
            self.infos_key = key.clone();
        }
        if self.sim_on {
            if self.sim_key != key || self.sim.is_none() {
                self.sim_key = key;
                match Sim::build(&self.ed.project, &self.ed.current) {
                    Ok(s) => self.sim = Some(s),
                    Err(e) => {
                        self.sim = None;
                        self.status = format!("Simulation error: {e}");
                    }
                }
            }
        } else {
            self.sim = None;
        }
        if let Some(sim) = self.sim.as_mut() {
            let ports = self
                .infos
                .iter()
                .flat_map(|i| i.abs.iter().map(|p| loc(*p)));
            sim.snapshot(wire_points(self.ed.circuit()).chain(ports));
        }
    }

    fn step_sim(&mut self, dt: f32) {
        if !(self.running && self.sim_on) {
            return;
        }
        self.accum += dt * self.hz;
        let n = self.accum.floor();
        self.accum -= n;
        if let Some(sim) = self.sim.as_mut() {
            for _ in 0..(n as u32).min(50) {
                sim.tick();
            }
        }
    }

    fn reset_sim(&mut self) {
        self.sim_key = (u64::MAX, String::new());
        self.accum = 0.0;
    }

    fn poke(&mut self, i: usize) {
        let def = self.ed.circuit().components[i].clone();
        if def.library != "Wiring"
            || def.part != "Pin"
            || def.options.boolean("output") == Ok(Some(true))
        {
            return;
        }
        let width = def.options.int("width", 1, 64).ok().flatten().unwrap_or(1);
        let cur = def
            .options
            .int("value", i64::MIN, i64::MAX)
            .ok()
            .flatten()
            .unwrap_or(0) as u64;
        let mask = if width >= 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        let next = if width == 1 {
            cur ^ 1
        } else {
            (cur + 1) & mask
        } as i64;
        self.ed.poke(i, next);
        let opts = self.ed.circuit().components[i].options.clone();
        if let Some(sim) = self.sim.as_mut() {
            sim.set_pin(i, &opts);
        }
    }

    // ---- geometry helpers -------------------------------------------------------------------------

    fn part_view<'a>(
        &'a self,
        i: usize,
        selected: bool,
        vals: &'a [Option<Value>],
    ) -> PartView<'a> {
        let info = &self.infos[i];
        PartView {
            def: &self.ed.circuit().components[i],
            name: &info.name,
            ports: &info.ports,
            port_vals: vals,
            selected,
            port_names: &info.port_names,
        }
    }

    fn port_vals(&self, i: usize) -> Vec<Option<Value>> {
        let Some(sim) = &self.sim else { return vec![] };
        self.infos[i]
            .abs
            .iter()
            .map(|p| sim.value_at(loc(*p)))
            .collect()
    }

    fn hit_part(&self, w: (f32, f32)) -> Option<usize> {
        let pos = Pos2::new(w.0, w.1);
        (0..self.infos.len()).rev().find(|&i| {
            let v = self.part_view(i, false, &[]);
            paint::world_bounds(v.def, &v).expand(2.0).contains(pos)
        })
    }

    fn hit_wire(&self, w: (f32, f32)) -> Option<usize> {
        let pos = Pos2::new(w.0, w.1);
        self.ed
            .circuit()
            .wires
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, wire)| {
                let a = Pos2::new(wire.a.0 as f32, wire.a.1 as f32);
                let ab = Pos2::new(wire.b.0 as f32, wire.b.1 as f32) - a;
                let t = if ab.length_sq() == 0.0 {
                    0.0
                } else {
                    ((pos - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
                };
                ((a + ab * t).distance(pos) < 4.0).then_some(i)
            })
    }

    fn hit_any(&self, w: (f32, f32)) -> Option<Sel> {
        self.hit_part(w)
            .map(Sel::Comp)
            .or_else(|| self.hit_wire(w).map(Sel::Wire))
    }

    fn select_in(&mut self, r: Rect, additive: bool) {
        let mut sel = if additive {
            self.ed.selection.clone()
        } else {
            vec![]
        };
        for i in 0..self.infos.len() {
            let v = self.part_view(i, false, &[]);
            if r.intersects(paint::world_bounds(v.def, &v)) && !sel.contains(&Sel::Comp(i)) {
                sel.push(Sel::Comp(i));
            }
        }
        for (i, w) in self.ed.circuit().wires.iter().enumerate() {
            let wr = Rect::from_two_pos(
                Pos2::new(w.a.0 as f32, w.a.1 as f32),
                Pos2::new(w.b.0 as f32, w.b.1 as f32),
            );
            if r.contains_rect(wr) && !sel.contains(&Sel::Wire(i)) {
                sel.push(Sel::Wire(i));
            }
        }
        self.ed.selection = sel;
    }

    /// Grid offset of the selection while it is being dragged.
    fn move_offset(&self) -> (i32, i32) {
        match (&self.drag, self.cursor_world) {
            (Drag::Move { origin }, Some(c)) => (snap(c.0 - origin.0), snap(c.1 - origin.1)),
            _ => (0, 0),
        }
    }

    // ---- canvas ------------------------------------------------------------------------------------

    fn canvas(&mut self, ui: &mut egui::Ui) {
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

    fn select_tool(
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
                Some(h) => self.ed.selection = vec![h],
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

    fn wire_tool(
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

    fn place_tool(
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

    fn draw_grid(&self, painter: &egui::Painter, rect: Rect, view: View, th: &Theme) {
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

    fn draw_wires(&self, painter: &egui::Painter, view: View, th: &Theme) {
        let c = self.ed.circuit();
        let (dx, dy) = self.move_offset();
        for (i, w) in c.wires.iter().enumerate() {
            let selected = self.ed.selection.contains(&Sel::Wire(i));
            let val = self.sim.as_ref().and_then(|s| s.value_at(loc(w.a)));
            let color = if selected {
                th.select
            } else {
                th.value_color(val)
            };
            let thick = if val.is_some_and(|v| v.width().get() > 1) {
                3.5
            } else {
                2.0
            };
            let (ox, oy) = if selected { (dx, dy) } else { (0, 0) };
            painter.line_segment(
                [
                    view.to_screen((w.a.0 + ox) as f32, (w.a.1 + oy) as f32),
                    view.to_screen((w.b.0 + ox) as f32, (w.b.1 + oy) as f32),
                ],
                Stroke::new((thick * view.zoom).max(1.5), color),
            );
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
                painter.circle_filled(
                    view.to_screen(p.0 as f32, p.1 as f32),
                    (3.5 * view.zoom).max(2.5),
                    th.value_color(val),
                );
            }
        }
    }

    fn draw_parts(&self, painter: &egui::Painter, view: View, th: &Theme) {
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

    // ---- keyboard ----------------------------------------------------------------------------------

    fn keys(&mut self, ctx: &egui::Context) {
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let (redo, undo, save, open, new, all) = ctx.input_mut(|i| {
            let redo =
                i.consume_key(cmd_shift, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y);
            (
                redo,
                !redo && i.consume_key(Modifiers::COMMAND, Key::Z),
                i.consume_key(Modifiers::COMMAND, Key::S),
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::N),
                i.consume_key(Modifiers::COMMAND, Key::A),
            )
        });
        if redo {
            self.ed.redo();
        }
        if undo {
            self.ed.undo();
        }
        if save {
            self.save(false);
        }
        if open {
            self.open_dialog();
        }
        if new {
            self.new_project();
        }
        if all {
            let n = self.ed.circuit().components.len();
            let m = self.ed.circuit().wires.len();
            self.ed.selection = (0..n).map(Sel::Comp).chain((0..m).map(Sel::Wire)).collect();
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (del, rot, esc) = ctx.input(|i| {
            (
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::R),
                i.key_pressed(Key::Escape),
            )
        });
        if del {
            self.ed.delete_selection();
        }
        if rot {
            if matches!(self.tool, Tool::Place(..)) {
                self.place_facing = crate::model::rotate_ccw(self.place_facing);
            } else {
                self.ed.rotate_selection();
            }
        }
        if esc {
            self.tool = Tool::Select;
            self.wire_chain = None;
            self.drag = Drag::None;
            self.ed.selection.clear();
        }
    }

    // ---- panels --------------------------------------------------------------------------------------

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.new_project();
                    ui.close();
                }
                if ui.button("Open…").clicked() {
                    self.open_dialog();
                    ui.close();
                }
                if ui.button("Save").clicked() {
                    self.save(false);
                    ui.close();
                }
                if ui.button("Save As…").clicked() {
                    self.save(true);
                    ui.close();
                }
                ui.separator();
                if ui.button("Open example: Half adder").clicked() {
                    self.load_example();
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(self.ed.can_undo(), egui::Button::new("Undo")).clicked() {
                    self.ed.undo();
                    ui.close();
                }
                if ui.add_enabled(self.ed.can_redo(), egui::Button::new("Redo")).clicked() {
                    self.ed.redo();
                    ui.close();
                }
                ui.separator();
                let has = !self.ed.selection.is_empty();
                if ui.add_enabled(has, egui::Button::new("Delete")).clicked() {
                    self.ed.delete_selection();
                    ui.close();
                }
                if ui.add_enabled(has, egui::Button::new("Rotate (R)")).clicked() {
                    self.ed.rotate_selection();
                    ui.close();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.radio_value(&mut self.mode, ThemeMode::Dark, "Dark theme").clicked()
                    | ui.radio_value(&mut self.mode, ThemeMode::Light, "Light theme").clicked()
                {
                    self.apply_theme(ui.ctx());
                }
                ui.checkbox(&mut self.show_grid, "Show grid");
                if ui.button("Reset view").clicked() {
                    self.views.remove(&self.ed.current);
                    ui.close();
                }
            });
            ui.menu_button("Simulate", |ui| {
                ui.checkbox(&mut self.sim_on, "Simulation enabled");
                let label = if self.running { "Pause clock" } else { "Run clock" };
                if ui.button(label).clicked() {
                    self.running = !self.running;
                    ui.close();
                }
                if ui.button("Step one tick").clicked() {
                    if let Some(s) = self.sim.as_mut() {
                        s.tick();
                    }
                    ui.close();
                }
                if ui.button("Reset simulation").clicked() {
                    self.reset_sim();
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    self.status = "rcirc: a beginner-friendly Rust port of Logisim Evolution (GPL-3.0-or-later).".into();
                    ui.close();
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let t = self.tool.clone();
            if ui
                .selectable_label(t == Tool::Select, "Select")
                .on_hover_text("Click, box-select, drag to move. Esc")
                .clicked()
            {
                self.tool = Tool::Select;
            }
            if ui
                .selectable_label(t == Tool::Poke, "Poke")
                .on_hover_text("Click an input Pin to flip it")
                .clicked()
            {
                self.tool = Tool::Poke;
            }
            if ui
                .selectable_label(t == Tool::Wire, "Wire")
                .on_hover_text("Drag to draw wires; click, click for chains")
                .clicked()
            {
                self.tool = Tool::Wire;
                self.wire_chain = None;
            }
            if let Tool::Place(_, part) = &t {
                ui.label(format!("Placing: {part} (R rotates, right-click stops)"));
            }
            ui.separator();
            ui.checkbox(&mut self.sim_on, "Simulate");
            let run = if self.running { "Pause" } else { "Run" };
            if ui
                .button(run)
                .on_hover_text("Run the clock continuously")
                .clicked()
            {
                self.running = !self.running;
            }
            if ui
                .button("Step")
                .on_hover_text("Advance the clock by one tick")
                .clicked()
            {
                if let Some(s) = self.sim.as_mut() {
                    s.tick();
                }
            }
            if ui
                .button("Reset")
                .on_hover_text("Restart the simulation")
                .clicked()
            {
                self.reset_sim();
            }
            ui.add(
                egui::DragValue::new(&mut self.hz)
                    .range(0.1..=60.0)
                    .speed(0.1)
                    .suffix(" ticks/s"),
            );
            if let Some(s) = &self.sim {
                ui.label(format!("tick {}", s.ticks));
                if s.oscillating() {
                    ui.colored_label(egui::Color32::from_rgb(240, 80, 80), "Oscillating circuit!");
                }
            }
        });
    }

    fn toolbox(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Parts");
            for lib in all_libraries() {
                egui::CollapsingHeader::new(lib.name)
                    .default_open(lib.name == "Gates" || lib.name == "Wiring")
                    .show(ui, |ui| {
                        for p in &lib.parts {
                            let selected = self.tool == Tool::Place(lib.name.into(), p.name.into());
                            if ui.selectable_label(selected, p.name).clicked() {
                                self.tool = Tool::Place(lib.name.into(), p.name.into());
                            }
                        }
                    });
            }
            ui.separator();
            ui.heading("Circuits");
            let names: Vec<String> = self
                .ed
                .project
                .circuits
                .iter()
                .map(|c| c.name.clone())
                .collect();
            for name in names {
                ui.horizontal(|ui| {
                    let main = if name == self.ed.project.main {
                        " (main)"
                    } else {
                        ""
                    };
                    if ui
                        .selectable_label(name == self.ed.current, format!("{name}{main}"))
                        .clicked()
                    {
                        self.ed.open_circuit(&name);
                    }
                    if name != self.ed.current
                        && ui
                            .small_button("Place")
                            .on_hover_text("Place this circuit as a part in the open circuit")
                            .clicked()
                    {
                        self.tool = Tool::Place(SUBCIRCUIT_LIBRARY.into(), name.clone());
                    }
                    if ui
                        .small_button("Main")
                        .on_hover_text("Make this the main circuit")
                        .clicked()
                    {
                        self.ed.set_main(&name);
                    }
                    if ui
                        .small_button("Del")
                        .on_hover_text("Delete this circuit")
                        .clicked()
                    {
                        if let Err(e) = self.ed.delete_circuit(&name) {
                            self.status = e;
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_circuit)
                        .hint_text("new circuit name")
                        .desired_width(110.0),
                );
                if ui.button("Add").clicked() {
                    let n = self.ed.add_circuit(&self.new_circuit.clone());
                    self.ed.open_circuit(&n);
                    self.new_circuit.clear();
                }
            });
        });
    }

    /// Option widgets for the selected part.
    fn attributes(&mut self, ui: &mut egui::Ui) {
        ui.heading("Attributes");
        ui.horizontal(|ui| {
            ui.label("Number format:");
            for b in Base::ALL {
                if ui.selectable_label(self.base == b, b.label()).clicked() {
                    self.base = b;
                    self.attr_buf.clear();
                }
            }
        });
        ui.separator();
        let [Sel::Comp(i)] = self.ed.selection[..] else {
            self.circuit_panel(ui);
            return;
        };
        let def = self.ed.circuit().components[i].clone();
        let sig = format!("{:?}{:?}", def.options, def.facing);
        let key = (self.ed.current.clone(), i, sig);
        if self.attr_for.as_ref() != Some(&key) {
            self.attr_buf.clear();
            self.attr_err.clear();
            self.attr_for = Some(key);
        }
        ui.strong(&self.infos[i].name);
        ui.label(format!("{}  at ({}, {})", def.library, def.at.0, def.at.1));
        let mut facing = def.facing;
        egui::ComboBox::from_label("Facing")
            .selected_text(format!("{facing:?}"))
            .show_ui(ui, |ui| {
                for d in [
                    Direction::East,
                    Direction::North,
                    Direction::West,
                    Direction::South,
                ] {
                    ui.selectable_value(&mut facing, d, format!("{d:?}"));
                }
            });
        if facing != def.facing {
            let mut d = def.clone();
            d.facing = facing;
            self.ed.replace_component(i, d);
            return;
        }
        ui.separator();
        for spec in self.specs_for(&def) {
            self.option_widget(ui, i, &def, &spec);
            ui.add_space(4.0);
        }
        if def.library == SUBCIRCUIT_LIBRARY {
            ui.label("Double-click the part to open this circuit.");
        }
    }

    fn specs_for(&self, def: &ComponentDef) -> Vec<OptionSpec> {
        if def.library != SUBCIRCUIT_LIBRARY {
            if let Ok(c) = def.build() {
                let specs = c.option_specs();
                if !specs.is_empty() {
                    return specs;
                }
            }
        }
        // no metadata: infer widgets from the current values
        def.options
            .0
            .iter()
            .map(|(k, v)| {
                let key: &'static str = Box::leak(k.clone().into_boxed_str());
                match v {
                    OptVal::Bool(b) => OptionSpec::boolean(key, key, *b),
                    OptVal::Int(n) => OptionSpec::int(key, key, i64::MIN, i64::MAX, *n),
                    OptVal::Str(s) => OptionSpec::string(key, key, s),
                }
            })
            .collect()
    }

    fn option_widget(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        def: &ComponentDef,
        spec: &OptionSpec,
    ) {
        let cur = def
            .options
            .0
            .get(spec.key)
            .cloned()
            .unwrap_or_else(|| spec.default.clone());
        let bufkey = format!("{idx}:{}", spec.key);
        match (&spec.kind, &cur) {
            (OptionKind::Bool, OptVal::Bool(b)) => {
                let mut v = *b;
                if ui.checkbox(&mut v, spec.label).changed() {
                    self.apply(idx, spec.key, OptVal::Bool(v));
                }
            }
            (OptionKind::Choice { names }, OptVal::Str(s)) => {
                let mut sel = s.clone();
                egui::ComboBox::from_label(spec.label)
                    .selected_text(sel.clone())
                    .show_ui(ui, |ui| {
                        for n in names {
                            ui.selectable_value(&mut sel, (*n).to_string(), *n);
                        }
                    });
                if &sel != s {
                    self.apply(idx, spec.key, OptVal::Str(sel));
                }
            }
            (OptionKind::Int { min, max }, OptVal::Int(n)) => {
                ui.label(spec.label);
                let buf = self
                    .attr_buf
                    .entry(bufkey.clone())
                    .or_insert_with(|| numfmt::format_int(*n, self.base));
                let r = ui.add(egui::TextEdit::singleline(buf).desired_width(150.0));
                let text = buf.clone();
                let parsed = numfmt::parse_int(&text).and_then(|v| {
                    if (*min..=*max).contains(&v) {
                        Ok(v)
                    } else {
                        Err(format!(
                            "must be between {} and {}",
                            numfmt::format_int(*min, self.base),
                            numfmt::format_int(*max, self.base)
                        ))
                    }
                });
                match parsed {
                    Ok(v) => {
                        ui.weak(format!("= {}", numfmt::other_bases(v, self.base)));
                        if r.lost_focus() && v != *n {
                            self.apply(idx, spec.key, OptVal::Int(v));
                        }
                    }
                    Err(e) => {
                        ui.colored_label(egui::Color32::from_rgb(240, 90, 90), e);
                    }
                }
            }
            (_, OptVal::Str(s)) => {
                ui.label(spec.label);
                let buf = self.attr_buf.entry(bufkey).or_insert_with(|| s.clone());
                let r = ui.add(egui::TextEdit::singleline(buf).desired_width(150.0));
                if r.lost_focus() && *buf != *s {
                    let v = buf.clone();
                    self.apply(idx, spec.key, OptVal::Str(v));
                }
            }
            _ => {
                ui.label(format!("{}: unsupported value", spec.label));
            }
        }
        if let Some(e) = self.attr_err.get(spec.key) {
            ui.colored_label(egui::Color32::from_rgb(240, 90, 90), e);
        }
    }

    fn apply(&mut self, idx: usize, key: &str, v: OptVal) {
        match self.ed.set_option(idx, key, v) {
            Ok(()) => {
                self.attr_err.remove(key);
            }
            Err(e) => {
                self.attr_err.insert(key.to_string(), e.to_string());
            }
        }
    }

    fn circuit_panel(&mut self, ui: &mut egui::Ui) {
        ui.strong(format!("Circuit: {}", self.ed.current));
        ui.label(format!(
            "{} parts, {} wires",
            self.ed.circuit().components.len(),
            self.ed.circuit().wires.len()
        ));
        ui.separator();
        ui.label("Ports of this circuit when used as a part:");
        let iface = self.ed.circuit().interface.clone();
        if iface.is_empty() {
            ui.weak("none yet. Give a Pin a label to make it a port.");
        }
        for p in iface {
            let kind = if p.input { "input" } else { "output" };
            ui.label(format!(
                "{}  ({kind}, {} bit{})",
                p.name,
                p.width,
                if p.width == 1 { "" } else { "s" }
            ));
        }
        ui.separator();
        ui.weak("Select a part to edit its attributes.");
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("zoom {:.0}%", self.view().zoom * 100.0));
                if let Some((x, y)) = self.hover {
                    ui.label(format!("({x}, {y})"));
                }
            });
        });
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let star = if self.ed.dirty() { "*" } else { "" };
        let title = format!(
            "rcirc - {}{star} [{}]",
            self.ed.project.metadata.name, self.ed.current
        );
        if title != self.title_sent {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title_sent = title;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.keys(&ctx);
        self.step_sim(ctx.input(|i| i.stable_dt));
        self.refresh_caches();
        self.update_title(&ctx);
        if self.running {
            ctx.request_repaint();
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("toolbox")
            .default_size(190.0)
            .show(ui, |ui| self.toolbox(ui));
        egui::Panel::right("attributes")
            .default_size(240.0)
            .show(ui, |ui| self.attributes(ui));
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
    }
}
