//! Menus, toolbars and side panels.

//! The egui application: panels, canvas interaction, simulation control and file handling.
//! Document logic lives in `model`; this file turns input into model calls and draws.

use crate::model::Sel;
use crate::numfmt::{self, Base};
use eframe::egui::{self};
use logisim_components::all_libraries;
use logisim_core::{Direction, OptVal, OptionKind, OptionSpec};
use logisim_format::{ComponentDef, SUBCIRCUIT_LIBRARY};

use crate::app::*;

impl App {
    // ---- panels --------------------------------------------------------------------------------------

    pub(crate) fn menu(&mut self, ui: &mut egui::Ui) {
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
                if ui.add_enabled(has, egui::Button::new("Cut")).clicked() {
                    self.ed.cut();
                    ui.close();
                }
                if ui.add_enabled(has, egui::Button::new("Copy")).clicked() {
                    self.ed.copy();
                    ui.close();
                }
                if ui.add_enabled(self.ed.has_clipboard(), egui::Button::new("Paste")).clicked() {
                    self.paste_clip();
                    ui.close();
                }
                if ui.add_enabled(has, egui::Button::new("Duplicate")).clicked() {
                    self.duplicate();
                    ui.close();
                }
                ui.separator();
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
                ui.menu_button("Theme", |ui| {
                    for t in crate::prefs::ThemeChoice::ALL {
                        ui.radio_value(&mut self.prefs.theme, t, t.label());
                    }
                });
                ui.menu_button("Text and zoom", |ui| {
                    ui.label(format!("Interface zoom {:.0}%  (Cmd +/-)", self.prefs.ui_scale * 100.0));
                    let mut s = self.prefs.ui_scale;
                    if ui.add(egui::Slider::new(&mut s, 0.7..=2.0).show_value(false)).changed() {
                        self.set_ui_scale(s);
                    }
                    ui.label(format!("Text size {:.0}%", self.prefs.font_scale * 100.0));
                    ui.add(egui::Slider::new(&mut self.prefs.font_scale, 0.8..=1.6).show_value(false));
                    if ui.button("Reset zoom and text size").clicked() {
                        self.prefs.ui_scale = 1.0;
                        self.prefs.font_scale = 1.0;
                    }
                });
                ui.checkbox(&mut self.show_grid, "Show grid");
                ui.checkbox(&mut self.colour_nets, "Colour nets");
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

    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let t = self.tool.clone();
            if ui
                .selectable_label(t == Tool::Select, "Select")
                .on_hover_text("Click or box-select, drag to move (wires follow). Clicking an input Pin flips it. Esc")
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

    pub(crate) fn toolbox(&mut self, ui: &mut egui::Ui) {
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
    pub(crate) fn attributes(&mut self, ui: &mut egui::Ui) {
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

    pub(crate) fn specs_for(&self, def: &ComponentDef) -> Vec<OptionSpec> {
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

    pub(crate) fn option_widget(
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

    pub(crate) fn apply(&mut self, idx: usize, key: &str, v: OptVal) {
        match self.ed.set_option(idx, key, v) {
            Ok(()) => {
                self.attr_err.remove(key);
            }
            Err(e) => {
                self.attr_err.insert(key.to_string(), e.to_string());
            }
        }
    }

    pub(crate) fn circuit_panel(&mut self, ui: &mut egui::Ui) {
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

    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
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

    pub(crate) fn update_title(&mut self, ctx: &egui::Context) {
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
