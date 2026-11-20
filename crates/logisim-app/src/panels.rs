//! Menus, toolbars and side panels.

//! The egui application: panels, canvas interaction, simulation control and file handling.
//! Document logic lives in `model`; this file turns input into model calls and draws.

use crate::model::Sel;
use crate::numfmt::{self, Base};
use eframe::egui::{self, Align2, Key};
use logisim_components::all_libraries;
use logisim_core::{Direction, OptVal, OptionKind, OptionSpec};
use logisim_format::{ComponentDef, SUBCIRCUIT_LIBRARY};

use crate::actions::Action;
use crate::app::*;
use crate::catalog::CatalogPart;

impl App {
    // ---- panels --------------------------------------------------------------------------------------

    /// One menu entry bound to an action (label, shortcut text, enabled state).
    fn menu_item(&mut self, ui: &mut egui::Ui, a: Action, enabled: bool) {
        let mut b = egui::Button::new(a.label());
        if let Some(sc) = a.shortcut() {
            b = b.shortcut_text(sc);
        }
        if ui.add_enabled(enabled, b).clicked() {
            self.run_action(a, ui.ctx());
            ui.close();
        }
    }

    fn menu_check(&mut self, ui: &mut egui::Ui, a: Action, on: bool) {
        let mut v = on;
        if ui.checkbox(&mut v, a.label()).changed() {
            self.run_action(a, ui.ctx());
        }
    }

    pub(crate) fn menu(&mut self, ui: &mut egui::Ui) {
        let has_sel = !self.ed.selection.is_empty();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                self.menu_item(ui, Action::New, true);
                self.menu_item(ui, Action::Open, true);
                ui.menu_button("Open recent", |ui| {
                    let recent = self.prefs.recent_files.clone();
                    if recent.is_empty() {
                        ui.weak("No recent files yet");
                    }
                    for f in recent {
                        let name = std::path::Path::new(&f).file_name().map_or(f.clone(), |n| n.to_string_lossy().into());
                        if ui.button(name).on_hover_text(&f).clicked() {
                            if self.confirm_discard() {
                                self.open_path(std::path::PathBuf::from(&f));
                            }
                            ui.close();
                        }
                    }
                });
                self.menu_item(ui, Action::Save, true);
                self.menu_item(ui, Action::SaveAs, true);
                ui.separator();
                self.menu_item(ui, Action::OpenExample, true);
                ui.separator();
                self.menu_item(ui, Action::Quit, true);
            });
            ui.menu_button("Edit", |ui| {
                let (u, r) = (self.ed.can_undo(), self.ed.can_redo());
                self.menu_item(ui, Action::Undo, u);
                self.menu_item(ui, Action::Redo, r);
                ui.separator();
                self.menu_item(ui, Action::Cut, has_sel);
                self.menu_item(ui, Action::Copy, has_sel);
                let clip = self.ed.has_clipboard();
                self.menu_item(ui, Action::Paste, clip);
                self.menu_item(ui, Action::Duplicate, has_sel);
                ui.separator();
                self.menu_item(ui, Action::Delete, has_sel);
                self.menu_item(ui, Action::Rotate, has_sel);
                self.menu_item(ui, Action::SelectAll, true);
            });
            ui.menu_button("View", |ui| {
                self.menu_item(ui, Action::FitView, true);
                self.menu_item(ui, Action::ResetView, true);
                ui.separator();
                let (g, c, m) = (self.show_grid, self.colour_nets, self.prefs.show_minimap);
                self.menu_check(ui, Action::ToggleGrid, g);
                self.menu_check(ui, Action::ToggleColourNets, c);
                self.menu_check(ui, Action::ToggleMinimap, m);
                ui.separator();
                ui.menu_button("Theme", |ui| {
                    for t in crate::prefs::ThemeChoice::ALL {
                        ui.radio_value(&mut self.prefs.theme, t, t.label());
                    }
                });
                ui.menu_button("Text and zoom", |ui| {
                    ui.label(format!("Interface zoom {:.0}%  ({}+ / {}−)", self.prefs.ui_scale * 100.0, mod_key(), mod_key()));
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
                self.menu_item(ui, Action::ResetLayout, true);
            });
            ui.menu_button("Simulate", |ui| {
                let on = self.sim_on;
                self.menu_check(ui, Action::SimToggle, on);
                let running = self.running;
                self.menu_check(ui, Action::RunToggle, running);
                self.menu_item(ui, Action::Step, true);
                self.menu_item(ui, Action::ResetSim, true);
                ui.separator();
            });
            ui.menu_button("Window", |ui| {
                use crate::windows::WindowGroup;
                let mut toggled: Option<&'static str> = None;
                for g in WindowGroup::ALL {
                    let mut any = false;
                    for w in self.windows.list.iter().filter(|w| w.group() == g) {
                        if !any {
                            ui.label(egui::RichText::new(g.label()).weak());
                            any = true;
                        }
                        let mut on = self.windows.open.contains(w.id());
                        let r = ui.checkbox(&mut on, w.title());
                        let r = if w.description().is_empty() { r } else { r.on_hover_text(w.description()) };
                        if r.changed() {
                            toggled = Some(w.id());
                        }
                    }
                }
                if self.windows.list.is_empty() {
                    ui.weak("No extra windows registered");
                }
                if let Some(id) = toggled {
                    self.windows.toggle(id);
                }
            });
            ui.menu_button("Help", |ui| {
                self.menu_item(ui, Action::CommandPalette, true);
                self.menu_item(ui, Action::Shortcuts, true);
                ui.separator();
                if ui.button("About rcirc").clicked() {
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

    /// One part in the toolbox: name (click to place, drag onto the canvas), favourite star.
    fn part_row(&mut self, ui: &mut egui::Ui, p: &CatalogPart) {
        let id = p.id();
        let fav = self.prefs.favourites.contains(&id);
        let selected = self.tool == Tool::Place(p.library.clone(), p.name.clone());
        ui.horizontal(|ui| {
            let star_w = crate::style::MIN_TARGET;
            let w = (ui.available_width() - star_w - ui.spacing().item_spacing.x - 1.0).max(60.0);
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(w, crate::style::MIN_TARGET),
                egui::Sense::click_and_drag(),
            );
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::Role::Button,
                    true,
                    format!("{} ({})", p.name, p.library),
                )
            });
            if ui.is_rect_visible(rect) {
                let vis = ui.visuals();
                let bg = if selected {
                    vis.selection.bg_fill
                } else if resp.hovered() || resp.has_focus() {
                    vis.widgets.hovered.bg_fill
                } else {
                    egui::Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, 6.0, bg);
                if resp.has_focus() {
                    ui.painter().rect_stroke(
                        rect,
                        6.0,
                        vis.selection.stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                let text = vis.text_color();
                ui.painter().text(
                    rect.left_center() + egui::vec2(10.0, 0.0),
                    Align2::LEFT_CENTER,
                    &p.name,
                    egui::TextStyle::Body.resolve(ui.style()),
                    text,
                );
            }
            if resp.drag_started() {
                resp.dnd_set_drag_payload(PartPayload {
                    library: p.library.clone(),
                    part: p.name.clone(),
                });
            }
            let resp = resp.on_hover_text(self.part_tooltip(&p.library, &p.name));
            if resp.clicked() {
                self.start_place(&p.library, &p.name);
            }
            let star = if fav { "★" } else { "☆" };
            let hint = if fav {
                "Remove from favourites"
            } else {
                "Add to favourites"
            };
            let sr = ui.add_sized(
                [star_w, crate::style::MIN_TARGET],
                egui::Button::new(star).frame(false),
            );
            sr.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::Role::CheckBox,
                    true,
                    fav,
                    format!("Favourite {}", p.name),
                )
            });
            if sr.on_hover_text(hint).clicked() {
                self.prefs.toggle_favourite(&id);
            }
        });
    }

    pub(crate) fn toolbox(&mut self, ui: &mut egui::Ui) {
        let search_id = egui::Id::unique("part_search");
        let r = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .id(search_id)
                .hint_text("Search parts…  (Ctrl/⌘ F)")
                .desired_width(ui.available_width()),
        );
        if self.focus_search {
            r.request_focus();
            self.focus_search = false;
        }
        if r.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            self.search.clear();
        }
        let query = self.search.trim().to_string();
        let parts = self.catalog.clone();
        let find = |id: &str| parts.iter().find(|p| p.id() == id).cloned();
        let matches: Vec<CatalogPart> = if query.is_empty() {
            vec![]
        } else {
            let mut scored: Vec<(i32, usize)> = parts
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    crate::fuzzy::score(&query, &p.name)
                        .or_else(|| {
                            crate::fuzzy::score(&query, &format!("{} {}", p.library, p.description))
                                .map(|s| s / 4 - 30)
                        })
                        .map(|s| (s, i))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            scored.into_iter().map(|(_, i)| parts[i].clone()).collect()
        };
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            if let Some(first) = matches.first() {
                self.start_place(&first.library, &first.name);
            }
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if !query.is_empty() {
                    if matches.is_empty() {
                        ui.weak(
                            "No part matches. Try the command palette for actions and circuits.",
                        );
                    }
                    for p in &matches {
                        self.part_row(ui, p);
                    }
                    return;
                }
                let favs: Vec<CatalogPart> = self
                    .prefs
                    .favourites
                    .iter()
                    .filter_map(|i| find(i))
                    .collect();
                if !favs.is_empty() {
                    ui.strong("Favourites");
                    for p in &favs {
                        self.part_row(ui, p);
                    }
                    ui.add_space(6.0);
                }
                let recents: Vec<CatalogPart> = self
                    .prefs
                    .recent_parts
                    .iter()
                    .filter_map(|i| find(i))
                    .take(5)
                    .collect();
                if !recents.is_empty() {
                    ui.strong("Recent");
                    for p in &recents {
                        self.part_row(ui, p);
                    }
                    ui.add_space(6.0);
                }
                for lib in all_libraries() {
                    egui::CollapsingHeader::new(egui::RichText::new(lib.name).strong())
                        .default_open(lib.name == "Gates" || lib.name == "Wiring")
                        .show(ui, |ui| {
                            for p in parts.iter().filter(|p| p.library == lib.name) {
                                self.part_row(ui, p);
                            }
                        });
                }
            });
    }

    /// The list of circuits of the project, with place / main / delete and "new circuit".
    pub(crate) fn circuits_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let names: Vec<String> = self
                    .ed
                    .project
                    .circuits
                    .iter()
                    .map(|c| c.name.clone())
                    .collect();
                for name in names {
                    ui.horizontal_wrapped(|ui| {
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
                            self.start_place(SUBCIRCUIT_LIBRARY, &name);
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
                            .desired_width(120.0),
                    );
                    if ui.button("Add").clicked() {
                        let n = self.ed.add_circuit(&self.new_circuit.clone());
                        self.ed.open_circuit(&n);
                        self.new_circuit.clear();
                    }
                });
            });
    }

    /// Tab strip of the project's circuits above the canvas.
    pub(crate) fn circuit_tabs(&mut self, ui: &mut egui::Ui) {
        let names: Vec<String> = self
            .ed
            .project
            .circuits
            .iter()
            .map(|c| c.name.clone())
            .collect();
        egui::ScrollArea::horizontal()
            .id_salt("circuit_tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for name in &names {
                        let active = *name == self.ed.current;
                        let label = if *name == self.ed.project.main {
                            format!("{name}  (main)")
                        } else {
                            name.clone()
                        };
                        let r = ui.selectable_label(active, label);
                        r.widget_info(|| {
                            egui::WidgetInfo::selected(egui::Role::Tab, true, active, name.clone())
                        });
                        if r.clicked() {
                            self.ed.open_circuit(name);
                        }
                    }
                    if ui.button("+").on_hover_text("New circuit").clicked() {
                        let n = self.ed.add_circuit("circuit");
                        self.ed.open_circuit(&n);
                    }
                });
            });
    }

    pub(crate) fn attributes(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Number format:");
            for (n, b) in Base::ALL.into_iter().enumerate() {
                let r = ui.selectable_label(self.base == b, b.label());
                if n == 0 {
                    self.attr_id = r.id;
                }
                if r.clicked() {
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

/// "Cmd" on macOS, "Ctrl" elsewhere.
pub(crate) fn mod_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    }
}
