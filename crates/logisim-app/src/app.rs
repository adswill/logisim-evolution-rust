//! The egui application: panels, canvas interaction, simulation control and file handling.
//! Document logic lives in `model`; this file turns input into model calls and draws.

use crate::actions::Action;
use crate::catalog::{self, CatalogPart};
use crate::examples;
use crate::model::{Editor, Sel, P};
use crate::nets::NetAnalysis;
use crate::numfmt::Base;
use crate::overlays::PaletteState;
use crate::paint::{self, PartView, ThemeMode, View};
use crate::prefs::{Prefs, MAX_RECENT_FILES, MAX_RECENT_PARTS};
use crate::sim::{wire_points, Sim};
use crate::style;
use eframe::egui::{self, Key, Modifiers, Pos2, Rect, Vec2};
use logisim_core::{Direction, Location, Port, PortKind, Value};
use logisim_format::{ComponentDef, ProjectFile, SUBCIRCUIT_LIBRARY};
use std::collections::HashMap;
use std::path::PathBuf;

/// Developer / screenshot switches from the command line.
#[derive(Default)]
pub struct DevFlags {
    pub theme: Option<crate::prefs::ThemeChoice>,
    pub palette: bool,
    pub shortcuts: bool,
}

/// Drag-and-drop payload when dragging a part from the toolbox.
#[derive(Clone)]
pub(crate) struct PartPayload {
    pub library: String,
    pub part: String,
}

pub(crate) const GRID: i32 = 10;

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Tool {
    Select,
    Poke,
    Wire,
    /// Place a part: (library, part); library `SUBCIRCUIT_LIBRARY` places a subcircuit.
    Place(String, String),
}

pub(crate) enum Drag {
    None,
    Move { origin: (f32, f32) },
    Box { origin: (f32, f32) },
    Wire { start: P },
}

/// Cached geometry of one placed part.
pub(crate) struct Info {
    pub(crate) name: String,
    pub(crate) ports: Vec<Port>,
    pub(crate) port_names: Vec<String>,
    pub(crate) abs: Vec<P>,
    pub(crate) error: Option<String>,
}

pub(crate) fn snap(v: f32) -> i32 {
    ((v / GRID as f32).round() as i32) * GRID
}

pub(crate) fn loc(p: P) -> Location {
    Location::new(p.0, p.1)
}

pub(crate) fn make_info(project: &ProjectFile, def: &ComponentDef) -> Info {
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
    pub(crate) ed: Editor,
    pub(crate) mode: ThemeMode,
    pub(crate) prefs: Prefs,
    pub(crate) catalog: Vec<CatalogPart>,
    pub(crate) palette: Option<PaletteState>,
    pub(crate) show_shortcuts: bool,
    pub(crate) search: String,
    pub(crate) focus_search: bool,
    pub(crate) fit_requested: bool,
    /// Dev/screenshot flag `--theme`: forces a theme without touching the saved preferences.
    pub(crate) theme_override: Option<crate::prefs::ThemeChoice>,
    pub(crate) canvas_id: egui::Id,
    pub(crate) attr_id: egui::Id,
    pub(crate) reset_layout: bool,
    pub(crate) prefs_saved: Prefs,
    pub(crate) style_key: Option<(ThemeMode, u32, u32)>,
    pub(crate) views: HashMap<String, View>,
    pub(crate) tool: Tool,
    pub(crate) place_facing: Direction,
    pub(crate) drag: Drag,
    pub(crate) wire_chain: Option<P>,
    pub(crate) cursor_world: Option<(f32, f32)>,
    pub(crate) hover: Option<P>,
    pub(crate) infos: Vec<Info>,
    pub(crate) infos_key: (u64, String),
    pub(crate) sim: Option<Sim>,
    pub(crate) sim_key: (u64, String),
    pub(crate) sim_on: bool,
    pub(crate) running: bool,
    pub(crate) hz: f32,
    pub(crate) accum: f32,
    pub(crate) base: Base,
    pub(crate) attr_buf: HashMap<String, String>,
    pub(crate) attr_for: Option<(String, usize, String)>,
    pub(crate) attr_err: HashMap<String, String>,
    pub(crate) new_circuit: String,
    pub(crate) status: String,
    pub(crate) show_grid: bool,
    pub(crate) colour_nets: bool,
    pub(crate) nets: NetAnalysis,
    pub(crate) point_net: HashMap<P, usize>,
    pub(crate) hover_net: Option<usize>,
    pub(crate) title_sent: String,
}

impl App {
    pub fn new(ctx: &egui::Context, file: Option<PathBuf>, example: bool, dev: &DevFlags) -> Self {
        let prefs = Prefs::load();
        let mode = style::resolve(prefs.theme, ctx);
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let mut app = App {
            ed: Editor::new(),
            mode,
            catalog: catalog::all_parts(),
            palette: None,
            show_shortcuts: false,
            search: String::new(),
            focus_search: false,
            fit_requested: false,
            theme_override: None,
            canvas_id: egui::Id::NULL,
            attr_id: egui::Id::NULL,
            reset_layout: false,
            prefs_saved: prefs.clone(),
            style_key: None,
            show_grid: prefs.show_grid,
            colour_nets: prefs.colour_nets,
            hz: prefs.hz,
            prefs,
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
            accum: 0.0,
            base: Base::Dec,
            attr_buf: HashMap::new(),
            attr_for: None,
            attr_err: HashMap::new(),
            new_circuit: String::new(),
            status: "Pick a part on the left, then click on the canvas to place it.".into(),
            nets: NetAnalysis::default(),
            point_net: HashMap::new(),
            hover_net: None,
            title_sent: String::new(),
        };
        if example {
            app.load_example();
        } else if let Some(p) = file {
            app.open_path(p);
        }
        app.theme_override = dev.theme;
        if dev.palette {
            app.open_palette();
        }
        app.show_shortcuts = dev.shortcuts;
        app.refresh_style(ctx);
        app
    }

    pub(crate) fn load_example(&mut self) {
        if self.confirm_discard() {
            self.ed = examples::half_adder();
            self.tool = Tool::Select;
            self.status =
                "Half adder: click the inputs A and B to flip them and watch Sum and Carry.".into();
        }
    }

    /// Re-apply the visual style when the theme or scale preferences changed.
    pub(crate) fn refresh_style(&mut self, ctx: &egui::Context) {
        self.mode = style::resolve(self.theme_override.unwrap_or(self.prefs.theme), ctx);
        let key = (
            self.mode,
            (self.prefs.ui_scale * 100.0) as u32,
            (self.prefs.font_scale * 100.0) as u32,
        );
        if self.style_key != Some(key) {
            style::apply(ctx, self.mode, self.prefs.ui_scale, self.prefs.font_scale);
            self.style_key = Some(key);
        }
    }

    /// Mirror the UI-state fields into the preferences and save when something changed.
    pub(crate) fn sync_prefs(&mut self) {
        self.prefs.show_grid = self.show_grid;
        self.prefs.colour_nets = self.colour_nets;
        self.prefs.hz = self.hz;
        if self.prefs != self.prefs_saved {
            self.prefs.save();
            self.prefs_saved = self.prefs.clone();
        }
    }

    pub(crate) fn set_ui_scale(&mut self, s: f32) {
        self.prefs.ui_scale = (s * 20.0).round() / 20.0;
        self.prefs.ui_scale = self.prefs.ui_scale.clamp(0.7, 2.0);
    }

    pub(crate) fn view(&self) -> View {
        self.views.get(&self.ed.current).copied().unwrap_or(View {
            origin: Vec2::new(300.0, 120.0),
            zoom: 1.0,
        })
    }

    // ---- files -----------------------------------------------------------------------------------

    pub(crate) fn confirm_discard(&self) -> bool {
        !self.ed.dirty()
            || rfd::MessageDialog::new()
                .set_title("Unsaved changes")
                .set_description("Discard the changes to this project?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes
    }

    pub(crate) fn new_project(&mut self) {
        if self.confirm_discard() {
            self.ed = Editor::new();
            self.status = "New project".into();
        }
    }

    pub(crate) fn open_dialog(&mut self) {
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

    pub(crate) fn open_path(&mut self, p: PathBuf) {
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
                self.remember_file(&p);
                self.ed = Editor::from_project(project, Some(p));
            }
            Err(e) => self.status = format!("Could not open {}: {e}", p.display()),
        }
    }

    pub(crate) fn save(&mut self, as_new: bool) {
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
                self.remember_file(&path);
                self.ed.path = Some(path);
                self.ed.mark_saved();
            }
            Err(e) => self.status = format!("Could not save: {e}"),
        }
    }

    // ---- caches and simulation --------------------------------------------------------------------

    pub(crate) fn refresh_caches(&mut self) {
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
            self.nets = self.ed.nets().clone();
            self.point_net = self
                .nets
                .nets
                .iter()
                .enumerate()
                .flat_map(|(i, n)| n.points.iter().map(move |p| (*p, i)))
                .collect();
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

    pub(crate) fn step_sim(&mut self, dt: f32) {
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

    pub(crate) fn reset_sim(&mut self) {
        self.sim_key = (u64::MAX, String::new());
        self.accum = 0.0;
    }

    pub(crate) fn is_input_pin(&self, i: usize) -> bool {
        let d = &self.ed.circuit().components[i];
        d.library == "Wiring" && d.part == "Pin" && d.options.boolean("output") != Ok(Some(true))
    }

    pub(crate) fn poke(&mut self, i: usize) {
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

    pub(crate) fn part_view<'a>(
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

    pub(crate) fn port_vals(&self, i: usize) -> Vec<Option<Value>> {
        let Some(sim) = &self.sim else { return vec![] };
        self.infos[i]
            .abs
            .iter()
            .map(|p| sim.value_at(loc(*p)))
            .collect()
    }

    pub(crate) fn hit_part(&self, w: (f32, f32)) -> Option<usize> {
        let pos = Pos2::new(w.0, w.1);
        (0..self.infos.len()).rev().find(|&i| {
            let v = self.part_view(i, false, &[]);
            paint::world_bounds(v.def, &v).expand(2.0).contains(pos)
        })
    }

    pub(crate) fn hit_wire(&self, w: (f32, f32)) -> Option<usize> {
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

    pub(crate) fn hit_any(&self, w: (f32, f32)) -> Option<Sel> {
        self.hit_part(w)
            .map(Sel::Comp)
            .or_else(|| self.hit_wire(w).map(Sel::Wire))
    }

    pub(crate) fn select_in(&mut self, r: Rect, additive: bool) {
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
    pub(crate) fn move_offset(&self) -> (i32, i32) {
        match (&self.drag, self.cursor_world) {
            (Drag::Move { origin }, Some(c)) => (snap(c.0 - origin.0), snap(c.1 - origin.1)),
            _ => (0, 0),
        }
    }

    pub(crate) fn paste_clip(&mut self) {
        let skipped = self.ed.paste(self.hover);
        if skipped > 0 {
            self.status =
                format!("{skipped} circuit instance(s) not pasted: they would contain themselves");
        }
    }

    pub(crate) fn duplicate(&mut self) {
        let skipped = self.ed.duplicate();
        if skipped > 0 {
            self.status = format!(
                "{skipped} circuit instance(s) not duplicated: they would contain themselves"
            );
        }
    }

    // ---- keyboard ----------------------------------------------------------------------------------

    /// Start placing a part (remembered in the recent list).
    pub(crate) fn start_place(&mut self, library: &str, part: &str) {
        if library != SUBCIRCUIT_LIBRARY {
            Prefs::push_recent(
                &mut self.prefs.recent_parts,
                &format!("{library}/{part}"),
                MAX_RECENT_PARTS,
            );
        }
        self.tool = Tool::Place(library.into(), part.into());
        self.place_facing = Direction::East;
    }

    pub(crate) fn remember_file(&mut self, path: &std::path::Path) {
        Prefs::push_recent(
            &mut self.prefs.recent_files,
            &path.to_string_lossy(),
            MAX_RECENT_FILES,
        );
    }

    /// Single entry point for every command (menus, shortcuts, palette).
    pub(crate) fn run_action(&mut self, a: Action, ctx: &egui::Context) {
        use crate::prefs::ThemeChoice;
        match a {
            Action::New => self.new_project(),
            Action::Open => self.open_dialog(),
            Action::Save => self.save(false),
            Action::SaveAs => self.save(true),
            Action::OpenExample => self.load_example(),
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::Undo => self.ed.undo(),
            Action::Redo => self.ed.redo(),
            Action::Cut => self.ed.cut(),
            Action::Copy => self.ed.copy(),
            Action::Paste => self.paste_clip(),
            Action::Duplicate => self.duplicate(),
            Action::Delete => self.ed.delete_selection(),
            Action::Rotate => {
                if matches!(self.tool, Tool::Place(..)) {
                    self.place_facing = crate::model::rotate_ccw(self.place_facing);
                } else {
                    self.ed.rotate_selection();
                }
            }
            Action::SelectAll => {
                let (n, m) = (
                    self.ed.circuit().components.len(),
                    self.ed.circuit().wires.len(),
                );
                self.ed.selection = (0..n).map(Sel::Comp).chain((0..m).map(Sel::Wire)).collect();
            }
            Action::ToolSelect => self.tool = Tool::Select,
            Action::ToolPoke => self.tool = Tool::Poke,
            Action::ToolWire => {
                self.tool = Tool::Wire;
                self.wire_chain = None;
            }
            Action::SimToggle => self.sim_on = !self.sim_on,
            Action::RunToggle => self.running = !self.running,
            Action::Step => {
                if let Some(s) = self.sim.as_mut() {
                    s.tick();
                }
            }
            Action::ResetSim => self.reset_sim(),
            Action::ToggleGrid => self.show_grid = !self.show_grid,
            Action::ToggleColourNets => self.colour_nets = !self.colour_nets,
            Action::ToggleMinimap => self.prefs.show_minimap = !self.prefs.show_minimap,
            Action::FitView => self.fit_requested = true,
            Action::ResetView => {
                self.views.remove(&self.ed.current);
            }
            Action::ZoomIn => self.set_ui_scale(self.prefs.ui_scale + 0.1),
            Action::ZoomOut => self.set_ui_scale(self.prefs.ui_scale - 0.1),
            Action::ThemeSystem => self.prefs.theme = ThemeChoice::System,
            Action::ThemeLight => self.prefs.theme = ThemeChoice::Light,
            Action::ThemeDark => self.prefs.theme = ThemeChoice::Dark,
            Action::ThemeHighContrast => self.prefs.theme = ThemeChoice::HighContrast,
            Action::ResetLayout => self.reset_layout = true,
            Action::FocusSearch => self.focus_search = true,
            Action::CommandPalette => self.open_palette(),
            Action::Shortcuts => self.show_shortcuts = !self.show_shortcuts,
        }
    }

    pub(crate) fn keys(&mut self, ctx: &egui::Context) {
        // clipboard events (the OS turns Cmd+C/X/V into these)
        let (copy, cut, paste) = ctx.input(|i| {
            let has = |f: fn(&egui::Event) -> bool| i.events.iter().any(f);
            (
                has(|e| matches!(e, egui::Event::Copy)),
                has(|e| matches!(e, egui::Event::Cut)),
                has(|e| matches!(e, egui::Event::Paste(_))),
            )
        });
        let typing = ctx.egui_wants_keyboard_input();
        if self.palette.is_some() {
            return; // the palette owns the keyboard while open
        }
        if !typing {
            if copy {
                self.run_action(Action::Copy, ctx);
            }
            if cut {
                self.run_action(Action::Cut, ctx);
            }
            if paste {
                self.run_action(Action::Paste, ctx);
            }
        }
        // table-driven shortcuts
        for &a in Action::ALL {
            let Some((m, k)) = a.binding() else { continue };
            if typing && a.needs_canvas_focus() {
                continue;
            }
            let hit = ctx.input_mut(|i| {
                i.consume_key(m, k) || (a == Action::ZoomIn && i.consume_key(m, Key::Plus))
            });
            if hit {
                self.run_action(a, ctx);
                break;
            }
        }
        if typing {
            return;
        }
        let (del, rot, esc, f6, shift) = ctx.input(|i| {
            (
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::R) && !i.modifiers.command,
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::F6),
                i.modifiers.shift,
            )
        });
        if del {
            self.run_action(Action::Delete, ctx);
        }
        if rot {
            self.run_action(Action::Rotate, ctx);
        }
        if esc {
            self.tool = Tool::Select;
            self.wire_chain = None;
            self.drag = Drag::None;
            self.ed.selection.clear();
        }
        if f6 {
            self.cycle_focus(ctx, shift);
        }
        // arrow keys nudge the selection (one grid step, Shift = ten)
        let step = if shift { 10 * GRID } else { GRID };
        for (key, dx, dy) in [
            (Key::ArrowLeft, -step, 0),
            (Key::ArrowRight, step, 0),
            (Key::ArrowUp, 0, -step),
            (Key::ArrowDown, 0, step),
        ] {
            if ctx.input_mut(|i| {
                i.consume_key(Modifiers::NONE, key) || i.consume_key(Modifiers::SHIFT, key)
            }) && !self.ed.selection.is_empty()
            {
                self.ed.move_selection(dx, dy);
            }
        }
    }

    /// F6 moves focus between the main areas: part search -> canvas -> attributes.
    pub(crate) fn cycle_focus(&mut self, ctx: &egui::Context, backwards: bool) {
        let order = [
            egui::Id::unique("part_search"),
            self.canvas_id,
            self.attr_id,
        ];
        let cur = order
            .iter()
            .position(|id| ctx.memory(|m| m.has_focus(*id)))
            .unwrap_or(1);
        let next = if backwards {
            (cur + order.len() - 1) % order.len()
        } else {
            (cur + 1) % order.len()
        };
        ctx.memory_mut(|m| m.request_focus(order[next]));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.refresh_style(&ctx);
        self.keys(&ctx);
        self.step_sim(ctx.input(|i| i.stable_dt));
        self.refresh_caches();
        self.update_title(&ctx);
        self.sync_prefs();
        if self.running {
            ctx.request_repaint();
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("toolbox")
            .default_size(230.0)
            .size_range(180.0..=380.0)
            .show(ui, |ui| self.toolbox(ui));
        egui::Panel::right("attributes")
            .default_size(260.0)
            .size_range(200.0..=420.0)
            .show(ui, |ui| self.attributes(ui));
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        self.palette_ui(&ctx);
        self.shortcuts_ui(&ctx);
    }
}
