//! The Simplified tab: minimal sum of products / product of sums and the Karnaugh map.

use super::pal::{describe, mono, Pal};
use super::state::Analyzer;
use eframe::egui::{self, Align2, Color32, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};
use logisim_analyze::implicant::{Form, Implicant, Minimal};
use logisim_analyze::kmap::KMap;

const CELL: f32 = 52.0;

pub fn show(ui: &mut egui::Ui, st: &mut Analyzer, pal: &Pal) {
    let names = st.model.output_bits();
    if names.is_empty() || st.model.table().input_count() == 0 {
        ui.label("Add at least one input and one output on the left first.");
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Output:");
        egui::ComboBox::from_id_salt("simp_out")
            .selected_text(&names[st.selected])
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    ui.selectable_value(&mut st.selected, i, n);
                }
            });
        ui.add_space(12.0);
        ui.selectable_value(&mut st.form, Form::SumOfProducts, "Sum of products")
            .on_hover_text("OR of AND terms, built from the rows that are 1");
        ui.selectable_value(&mut st.form, Form::ProductOfSums, "Product of sums")
            .on_hover_text("AND of OR terms, built from the rows that are 0");
    });
    ui.add_space(6.0);
    let out = st.selected;

    if st.needs_force() {
        ui.label(format!(
            "This table has {} inputs. Simplifying more than {} inputs can take a while, so it only runs when you ask.",
            st.model.table().input_count(),
            Analyzer::auto_limit()
        ));
        if ui.button("Simplify anyway").clicked() {
            st.forced = true;
        }
        return;
    }
    let Some(simp) = st.simplified(out) else {
        return;
    };
    let notation = st.notation;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{} =", names[out]))
                .strong()
                .monospace(),
        );
        ui.label(
            RichText::new(simp.expr.to_text(notation))
                .monospace()
                .size(16.0)
                .color(pal.accent),
        );
    });
    ui.horizontal(|ui| {
        if ui
            .button("Use as this output's expression")
            .on_hover_text(
                "Stores it in the Expressions tab; building a circuit then uses exactly this",
            )
            .clicked()
        {
            st.use_simplified(out);
        }
        if ui.button("Copy").clicked() {
            ui.ctx().copy_text(simp.expr.to_text(notation));
        }
    });
    ui.add_space(8.0);

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| match st.kmap() {
            Some(k) => kmap(ui, st, pal, &k, out, &simp.minimal),
            None => {
                ui.label(format!(
                    "A Karnaugh map shows 1 to {} inputs; this table has {}.",
                    Analyzer::kmap_limit(),
                    st.model.table().input_count()
                ));
            }
        });
}

fn group_letter(i: usize) -> String {
    let a = (b'a' + (i % 26) as u8) as char;
    if i < 26 {
        a.to_string()
    } else {
        format!("{a}{}", i / 26)
    }
}

fn kmap(ui: &mut egui::Ui, st: &Analyzer, pal: &Pal, k: &KMap, out: usize, minimal: &Minimal) {
    let names = st.model.input_bits();
    let rv = k.row_vars();
    let side = names[..rv].join("");
    let top = names[rv..].join("");
    let label_w = 60.0;
    let head_h = 40.0;
    let size = Vec2::new(
        label_w + CELL * k.cols() as f32 + 8.0,
        head_h + CELL * k.rows() as f32 + 8.0,
    );
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect.expand(2.0));
    let origin = rect.min + Vec2::new(label_w, head_h);
    // corner: side variables over top variables
    let corner = Rect::from_min_size(rect.min, Vec2::new(label_w, head_h));
    p.rect_filled(corner.shrink(1.0), 3.0, pal.header_bg);
    p.text(
        corner.left_bottom() + Vec2::new(6.0, -6.0),
        Align2::LEFT_BOTTOM,
        if side.is_empty() {
            "·".into()
        } else {
            side.clone()
        },
        mono(12.0),
        pal.text,
    );
    p.text(
        corner.right_top() + Vec2::new(-6.0, 6.0),
        Align2::RIGHT_TOP,
        top.clone(),
        mono(12.0),
        pal.text,
    );
    for c in 0..k.cols() {
        let r = Rect::from_min_size(
            Pos2::new(origin.x + c as f32 * CELL, rect.min.y),
            Vec2::new(CELL, head_h),
        );
        p.rect_filled(r.shrink(1.0), 3.0, pal.header_bg);
        p.text(
            r.center(),
            Align2::CENTER_CENTER,
            k.col_label(c),
            mono(14.0),
            pal.text,
        );
    }
    for r in 0..k.rows() {
        let rr = Rect::from_min_size(
            Pos2::new(rect.min.x, origin.y + r as f32 * CELL),
            Vec2::new(label_w, CELL),
        );
        p.rect_filled(rr.shrink(1.0), 3.0, pal.header_bg);
        p.text(
            rr.center(),
            Align2::CENTER_CENTER,
            if rv == 0 { "·".into() } else { k.row_label(r) },
            mono(14.0),
            pal.text,
        );
    }
    let cell_rect = |r: usize, c: usize| {
        Rect::from_min_size(
            Pos2::new(origin.x + c as f32 * CELL, origin.y + r as f32 * CELL),
            Vec2::splat(CELL),
        )
    };

    // which groups cover each cell, for the corner letters
    let groups = k.groups(&minimal.implicants);
    for r in 0..k.rows() {
        for c in 0..k.cols() {
            let row = k.table_row(r, c);
            let e = st.model.table().get(row, out);
            let cr = cell_rect(r, c);
            p.rect_filled(cr.shrink(2.0), 4.0, pal.entry_fill(e));
            p.rect_stroke(
                cr.shrink(2.0),
                4.0,
                Stroke::new(1.0, pal.grid),
                StrokeKind::Inside,
            );
            p.text(
                cr.center(),
                Align2::CENTER_CENTER,
                e.to_char().to_string(),
                mono(20.0),
                pal.entry_text(e),
            );
            p.text(
                cr.left_top() + Vec2::new(6.0, 5.0),
                Align2::LEFT_TOP,
                row.to_string(),
                mono(9.0),
                pal.dim,
            );
            let letters: String = groups
                .iter()
                .enumerate()
                .filter(|(_, g)| g.cells.contains(&(r, c)))
                .map(|(i, _)| group_letter(i))
                .collect::<Vec<_>>()
                .join("");
            if !letters.is_empty() {
                p.text(
                    cr.right_bottom() - Vec2::new(5.0, 3.0),
                    Align2::RIGHT_BOTTOM,
                    letters,
                    mono(11.0),
                    pal.text,
                );
            }
        }
    }
    // group outlines (a group that wraps around the edge is drawn in pieces)
    for (i, g) in groups.iter().enumerate() {
        let colour = pal.groups[i % pal.groups.len()];
        let inset = 4.0 + (i % 3) as f32 * 2.5;
        for rc in &g.rects {
            for (r0, h) in split_wrap(rc.row, rc.height, k.rows()) {
                for (c0, w) in split_wrap(rc.col, rc.width, k.cols()) {
                    let area = Rect::from_min_max(
                        cell_rect(r0, c0).min,
                        cell_rect(r0 + h - 1, c0 + w - 1).max,
                    )
                    .shrink(inset);
                    p.rect_filled(
                        area,
                        8.0,
                        Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), 36),
                    );
                    p.rect_stroke(area, 8.0, Stroke::new(2.5, colour), StrokeKind::Inside);
                }
            }
        }
    }

    ui.add_space(8.0);
    if groups.is_empty() {
        let msg = match minimal.form {
            Form::SumOfProducts => "No row is 1, so the output is always 0.",
            Form::ProductOfSums => "No row is 0, so the output is always 1.",
        };
        ui.label(msg);
        return;
    }
    ui.label(RichText::new("Groups").strong());
    for (i, imp) in minimal.implicants.iter().enumerate() {
        legend_row(ui, pal, st, minimal, i, *imp, &groups[i]);
    }
}

/// Split a wrap-around run (start, length) on a ring into plain runs.
fn split_wrap(start: usize, len: usize, size: usize) -> Vec<(usize, usize)> {
    if start + len <= size {
        vec![(start, len)]
    } else {
        vec![(start, size - start), (0, len - (size - start))]
    }
}

fn legend_row(
    ui: &mut egui::Ui,
    pal: &Pal,
    st: &Analyzer,
    minimal: &Minimal,
    i: usize,
    imp: Implicant,
    g: &logisim_analyze::kmap::Group,
) {
    let names = st.model.input_bits();
    let term = Minimal {
        form: minimal.form,
        implicants: vec![imp],
    }
    .to_expr(&names);
    let colour = pal.groups[i % pal.groups.len()];
    ui.horizontal(|ui| {
        let (r, resp) = ui.allocate_exact_size(Vec2::new(26.0, 22.0), Sense::hover());
        ui.painter()
            .rect_stroke(r, 5.0, Stroke::new(2.5, colour), StrokeKind::Inside);
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            group_letter(i),
            mono(13.0),
            pal.text,
        );
        let _ = describe(resp, || format!("Group {}", group_letter(i)));
        ui.label(RichText::new(term.to_text(st.notation)).monospace());
        let n = g.cells.len();
        ui.label(
            RichText::new(format!("covers {n} row{}", if n == 1 { "" } else { "s" }))
                .color(pal.dim),
        );
    });
}
