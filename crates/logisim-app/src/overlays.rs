//! Floating overlays: command palette and keyboard-shortcut cheat sheet.

use crate::actions::{Action, EXTRA_HINTS};
use crate::app::*;
use crate::catalog;
use crate::palette::{self, Entry, Target};
use eframe::egui::{self, Align2, Key, Modifiers, RichText};
use logisim_format::SUBCIRCUIT_LIBRARY;

pub(crate) struct PaletteState {
    pub query: String,
    pub selected: usize,
    pub entries: Vec<Entry>,
    pub just_opened: bool,
}

impl App {
    pub(crate) fn open_palette(&mut self) {
        let circuits: Vec<String> = self
            .ed
            .project
            .circuits
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let entries = palette::entries(&self.catalog, &circuits);
        self.palette = Some(PaletteState {
            query: String::new(),
            selected: 0,
            entries,
            just_opened: true,
        });
    }

    pub(crate) fn palette_ui(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.palette.take() else {
            return;
        };
        let ranked = palette::rank(&st.entries, &st.query, &self.prefs.recent_parts, 12);
        st.selected = st.selected.min(ranked.len().saturating_sub(1));
        let (up, down, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if up {
            st.selected = st.selected.saturating_sub(1);
        }
        if down && st.selected + 1 < ranked.len() {
            st.selected += 1;
        }
        let mut chosen: Option<Target> = enter
            .then(|| {
                ranked
                    .get(st.selected)
                    .map(|&i| st.entries[i].target.clone())
            })
            .flatten();
        let mut close = esc;
        egui::Window::new("Command palette")
            .title_bar(false)
            .anchor(Align2::CENTER_TOP, [0.0, 70.0])
            .fixed_size([520.0, 0.0])
            .collapsible(false)
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut st.query)
                        .hint_text("Type a command, a part or a circuit…")
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Heading),
                );
                if st.just_opened {
                    r.request_focus();
                    st.just_opened = false;
                }
                if r.changed() {
                    st.selected = 0;
                }
                ui.add_space(4.0);
                if ranked.is_empty() {
                    ui.weak("Nothing matches. Try a part name (\"and\"), an action (\"save\") or a circuit name.");
                }
                for (row, &i) in ranked.iter().enumerate() {
                    let e = &st.entries[i];
                    let resp = ui
                        .scope(|ui| {
                            ui.set_min_height(crate::style::MIN_TARGET);
                            ui.horizontal(|ui| {
                                let selected = row == st.selected;
                                let r = ui.selectable_label(selected, RichText::new(&e.label));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.weak(e.detail.chars().take(60).collect::<String>());
                                });
                                r
                            })
                            .inner
                        })
                        .inner;
                    if resp.clicked() {
                        chosen = Some(e.target.clone());
                    }
                    if row == st.selected {
                        resp.scroll_to_me(None);
                    }
                }
                ui.add_space(2.0);
                ui.weak("↑↓ choose · Enter run · Esc close");
            });
        if let Some(t) = chosen {
            self.run_target(t, ctx);
            close = true;
        }
        if !close {
            self.palette = Some(st);
        }
    }

    fn run_target(&mut self, t: Target, ctx: &egui::Context) {
        match t {
            Target::Action(a) => self.run_action(a, ctx),
            Target::Part { library, part } => self.start_place(&library, &part),
            Target::OpenCircuit(n) => self.ed.open_circuit(&n),
            Target::PlaceCircuit(n) => self.start_place(SUBCIRCUIT_LIBRARY, &n),
        }
    }

    pub(crate) fn shortcuts_ui(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }
        let mut open = true;
        egui::Window::new("Keyboard shortcuts")
            .open(&mut open)
            .collapsible(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(480.0)
                    .show(ui, |ui| {
                        for cat in ["File", "Edit", "Tools", "Simulate", "View"] {
                            ui.strong(cat);
                            egui::Grid::new(format!("sc_{cat}"))
                                .num_columns(2)
                                .spacing([24.0, 4.0])
                                .show(ui, |ui| {
                                    for a in Action::ALL.iter().filter(|a| a.category() == cat) {
                                        ui.label(a.label());
                                        ui.monospace(a.shortcut().unwrap_or_else(|| "—".into()));
                                        ui.end_row();
                                    }
                                });
                            ui.add_space(6.0);
                        }
                        ui.strong("Mouse and keyboard");
                        egui::Grid::new("sc_extra")
                            .num_columns(2)
                            .spacing([24.0, 4.0])
                            .show(ui, |ui| {
                                for (k, d) in EXTRA_HINTS {
                                    ui.monospace(*k);
                                    ui.label(*d);
                                    ui.end_row();
                                }
                            });
                    });
            });
        self.show_shortcuts = open;
    }

    /// Descriptions for tooltips: part help plus how to use it.
    pub(crate) fn part_tooltip(&self, library: &str, part: &str) -> String {
        format!("{}\n\nClick, then click the canvas to place. Or drag it onto the canvas. R rotates while placing.", catalog::describe(library, part))
    }
}
