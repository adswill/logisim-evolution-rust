//! Expressions: one editable line per output bit, in the chosen notation.

use super::pal::{mono, Pal};
use super::state::Analyzer;
use eframe::egui::{self, RichText, TextEdit};
use logisim_analyze::expr::Notation;
use std::collections::HashMap;

/// Text being typed, per output bit, while its field has focus.
#[derive(Default)]
pub struct ExprBuffers {
    typing: HashMap<usize, String>,
}

pub fn show(ui: &mut egui::Ui, st: &mut Analyzer, pal: &Pal, bufs: &mut ExprBuffers) {
    ui.horizontal(|ui| {
        ui.label("Notation:");
        egui::ComboBox::from_id_salt("notation")
            .selected_text(st.notation.label())
            .width(300.0)
            .show_ui(ui, |ui| {
                for n in Notation::ALL {
                    ui.selectable_value(&mut st.notation, n, n.label());
                }
            });
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new(
            "Type an expression and press Enter. Write several terms next to each other for AND, for example  A ~B + C.  Words work too: A AND NOT B.  You can also type  Y = A ^ B  to set output Y.",
        )
        .color(pal.dim),
    );
    ui.add_space(8.0);
    let names = st.model.output_bits();
    if names.is_empty() {
        ui.label("Add an output on the left first.");
        return;
    }
    let texts: Vec<Option<String>> = st.expression_texts().to_vec();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (o, name) in names.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_sized([86.0, 24.0], egui::Label::new(RichText::new(format!("{name} =")).strong().monospace()));
                let shown = texts[o].clone().unwrap_or_default();
                let mut buf = bufs.typing.get(&o).cloned().unwrap_or_else(|| shown.clone());
                let hint = if texts[o].is_none() {
                    "Too many inputs to simplify automatically. Use the Simplified tab, or type an expression."
                } else {
                    "Type an expression"
                };
                let resp = ui.add(
                    TextEdit::singleline(&mut buf)
                        .font(mono(15.0))
                        .hint_text(hint)
                        .desired_width(ui.available_width() - 90.0),
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::accesskit::Role::TextInput, true, format!("Expression for output {name}"))
                });
                if resp.gained_focus() {
                    // selecting everything makes "click and type" replace the old expression
                    let mut state = egui::text_edit::TextEditState::load(ui.ctx(), resp.id).unwrap_or_default();
                    let end = buf.chars().count();
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(end))));
                    state.store(ui.ctx(), resp.id);
                }
                if resp.has_focus() {
                    bufs.typing.insert(o, buf.clone());
                }
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter || (resp.lost_focus() && buf != shown) {
                    st.set_expression_text(o, &buf);
                    if st.expr_problems[o].is_none() {
                        bufs.typing.remove(&o);
                    } else {
                        bufs.typing.insert(o, buf.clone());
                    }
                } else if resp.lost_focus() {
                    bufs.typing.remove(&o);
                }
                if ui.button("Copy").on_hover_text("Copy this expression to the clipboard").clicked() {
                    ui.ctx().copy_text(shown.clone());
                }
            });
            if let Some(p) = &st.expr_problems[o] {
                problem(ui, pal, &p.text, &p.error);
            }
            ui.add_space(4.0);
        }
    });
}

/// Show an error with the offending characters marked underneath.
fn problem(ui: &mut egui::Ui, pal: &Pal, text: &str, e: &logisim_analyze::parser::ParseError) {
    ui.horizontal(|ui| {
        ui.add_space(92.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(format!("Problem: {}", e.message)).color(pal.error));
            if e.length > 0 || e.offset < text.chars().count() {
                let marker: String = text
                    .chars()
                    .enumerate()
                    .map(|(i, _)| {
                        if i >= e.offset && i < e.offset + e.length.max(1) {
                            '^'
                        } else {
                            ' '
                        }
                    })
                    .collect();
                ui.label(RichText::new(text).monospace().color(pal.dim));
                ui.label(RichText::new(marker).monospace().color(pal.error));
            }
        });
    });
}
