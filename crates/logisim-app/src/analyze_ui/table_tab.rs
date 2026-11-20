//! The truth table editor: click an output cell to cycle 0, 1, x.

use super::pal::{describe, mono, Pal};
use super::state::Analyzer;
use eframe::egui::{self, Align2, Sense, Stroke, StrokeKind, Vec2};
use logisim_analyze::table::Entry;

const CELL_W: f32 = 30.0;
const ROW_H: f32 = 24.0;
const INDEX_W: f32 = 54.0;

pub fn show(ui: &mut egui::Ui, st: &mut Analyzer, pal: &Pal) {
    let t = st.model.table();
    let (n_in, n_out, rows) = (t.input_count(), t.output_count(), t.row_count());
    if n_in == 0 || n_out == 0 {
        ui.add_space(12.0);
        ui.label("Add at least one input and one output on the left to get a truth table.");
        return;
    }
    ui.label(
        egui::RichText::new(
            "Click an output cell to change it: 0, then 1, then x (don't care, the minimiser may use it either way).",
        )
        .color(pal.dim),
    );
    ui.add_space(4.0);
    let in_names = st.model.input_bits();
    let out_names = st.model.output_bits();
    let total_w = INDEX_W + CELL_W * (n_in + n_out) as f32 + 24.0;

    let mut clicked: Option<(usize, usize)> = None;
    let mut fill: Option<(usize, Entry)> = None;

    egui::ScrollArea::horizontal()
        .id_salt("table_h")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(total_w);
            // header
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                header_cell(ui, pal, "row", INDEX_W, false);
                for n in &in_names {
                    header_cell(ui, pal, n, CELL_W, false);
                }
                ui.add_space(12.0);
                for (o, n) in out_names.iter().enumerate() {
                    let r = header_cell(ui, pal, n, CELL_W, true);
                    r.context_menu(|ui| {
                        ui.label(format!("Set every row of {n} to"));
                        for (label, e) in [
                            ("0", Entry::Zero),
                            ("1", Entry::One),
                            ("x (don't care)", Entry::DontCare),
                        ] {
                            if ui.button(label).clicked() {
                                fill = Some((o, e));
                                ui.close();
                            }
                        }
                    });
                    r.on_hover_text(format!("{n}: right-click to set the whole column"));
                }
            });
            ui.add_space(2.0);
            egui::ScrollArea::vertical()
                .id_salt("table_v")
                .auto_shrink([false, false])
                .show_rows(ui, ROW_H, rows, |ui, range| {
                    for row in range {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(0.0, 0.0);
                            let (rect, _) = ui.allocate_exact_size(
                                Vec2::new(INDEX_W, ROW_H - 2.0),
                                Sense::hover(),
                            );
                            ui.painter().text(
                                rect.right_center() - Vec2::new(8.0, 0.0),
                                Align2::RIGHT_CENTER,
                                row.to_string(),
                                mono(13.0),
                                pal.dim,
                            );
                            for c in 0..n_in {
                                let bit = st.model.table().input_bit(row, c);
                                let (rect, _) = ui.allocate_exact_size(
                                    Vec2::new(CELL_W, ROW_H - 2.0),
                                    Sense::hover(),
                                );
                                ui.painter().text(
                                    rect.center(),
                                    Align2::CENTER_CENTER,
                                    if bit { "1" } else { "0" },
                                    mono(15.0),
                                    pal.text,
                                );
                            }
                            ui.add_space(12.0);
                            for o in 0..n_out {
                                let e = st.model.table().get(row, o);
                                let (rect, resp) = ui.allocate_exact_size(
                                    Vec2::new(CELL_W - 2.0, ROW_H - 2.0),
                                    Sense::click(),
                                );
                                let resp = describe(resp, || {
                                    format!(
                                        "Row {row}, output {}: {}. Activate to change.",
                                        out_names[o],
                                        e.to_char()
                                    )
                                });
                                let hot = resp.hovered() || resp.has_focus();
                                ui.painter().rect_filled(rect, 3.0, pal.entry_fill(e));
                                let stroke = if resp.has_focus() {
                                    Stroke::new(2.0, pal.accent)
                                } else if hot {
                                    Stroke::new(1.5, pal.text)
                                } else {
                                    Stroke::new(1.0, pal.grid)
                                };
                                ui.painter()
                                    .rect_stroke(rect, 3.0, stroke, StrokeKind::Inside);
                                ui.painter().text(
                                    rect.center(),
                                    Align2::CENTER_CENTER,
                                    e.to_char().to_string(),
                                    mono(15.0),
                                    pal.entry_text(e),
                                );
                                if resp.clicked() {
                                    clicked = Some((row, o));
                                }
                                ui.add_space(2.0);
                            }
                        });
                    }
                });
        });

    if let Some((row, o)) = clicked {
        st.cycle_entry(row, o);
    }
    if let Some((o, e)) = fill {
        st.fill_column(o, e);
    }
}

fn header_cell(
    ui: &mut egui::Ui,
    pal: &Pal,
    text: &str,
    w: f32,
    interactive: bool,
) -> egui::Response {
    let sense = if interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 36.0), sense);
    ui.painter()
        .rect_filled(rect.shrink(1.0), 3.0, pal.header_bg);
    // long names are shortened (full name in the tooltip); a bit index goes on its own line
    let (base, idx) = match text.find('[') {
        Some(i) if w <= CELL_W => (&text[..i], &text[i..]),
        _ => (text, ""),
    };
    let base: String = if w <= CELL_W && base.chars().count() > 4 {
        format!("{}…", base.chars().take(3).collect::<String>())
    } else {
        base.to_string()
    };
    let font = egui::FontId::proportional(12.0);
    if idx.is_empty() {
        ui.painter()
            .text(rect.center(), Align2::CENTER_CENTER, base, font, pal.text);
    } else {
        ui.painter().text(
            rect.center() - Vec2::new(0.0, 7.0),
            Align2::CENTER_CENTER,
            base,
            font.clone(),
            pal.text,
        );
        ui.painter().text(
            rect.center() + Vec2::new(0.0, 8.0),
            Align2::CENTER_CENTER,
            idx,
            font,
            pal.dim,
        );
    }
    if w <= CELL_W {
        resp.on_hover_text(text)
    } else {
        resp
    }
}
