//! Left panel: the input and output variables.

use super::pal::Pal;
use super::state::{Analyzer, Side};
use eframe::egui::{self, RichText, TextEdit};
use logisim_analyze::var::Var;
use std::collections::HashMap;

#[derive(Default)]
pub struct VarBuffers {
    /// Name being edited, keyed by (side, variable name when editing started).
    names: HashMap<(bool, String), String>,
    pub new_input: String,
    pub new_output: String,
    pub error: Option<String>,
}

pub fn show(ui: &mut egui::Ui, st: &mut Analyzer, pal: &Pal, b: &mut VarBuffers) {
    section(ui, st, pal, b, Side::Input);
    ui.add_space(10.0);
    section(ui, st, pal, b, Side::Output);
    if let Some(e) = &b.error {
        ui.add_space(8.0);
        ui.label(RichText::new(e).color(pal.error));
    }
}

fn section(ui: &mut egui::Ui, st: &mut Analyzer, pal: &Pal, b: &mut VarBuffers, side: Side) {
    let (title, list) = match side {
        Side::Input => ("Inputs", st.model.inputs().clone()),
        Side::Output => ("Outputs", st.model.outputs().clone()),
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong().size(15.0));
        ui.label(
            RichText::new(format!("{} of {} bits", list.bit_count(), list.max_bits()))
                .color(pal.dim),
        );
    });
    let count = list.vars().len();
    for (i, v) in list.vars().iter().enumerate() {
        ui.horizontal(|ui| {
            let key = (side == Side::Input, v.name.clone());
            let mut name = b.names.get(&key).cloned().unwrap_or_else(|| v.name.clone());
            let resp = ui.add(TextEdit::singleline(&mut name).desired_width(70.0));
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::TextInput, true, format!("{title} variable name")));
            if resp.has_focus() {
                b.names.insert(key.clone(), name.clone());
            }
            let mut width = v.width;
            let wr = ui.add(egui::DragValue::new(&mut width).range(1..=32).suffix(if v.width == 1 { " bit" } else { " bits" }));
            wr.clone().on_hover_text("Number of bits (more than 1 makes a bus: its bits are listed as Name[2], Name[1], Name[0])");
            let mut commit = None;
            if resp.lost_focus() {
                b.names.remove(&key);
                if name != v.name {
                    commit = Some(Var::new(name.trim(), v.width));
                }
            }
            if wr.changed() {
                commit = Some(Var::new(&v.name, width));
            }
            if let Some(nv) = commit {
                match st.replace_var(side, &v.name, nv) {
                    Ok(()) => b.error = None,
                    Err(e) => b.error = Some(e),
                }
            }
            if ui.add_enabled(i > 0, egui::Button::new("↑")).on_hover_text("Move up").clicked() {
                st.move_var(side, &v.name, -1);
            }
            if ui.add_enabled(i + 1 < count, egui::Button::new("↓")).on_hover_text("Move down").clicked() {
                st.move_var(side, &v.name, 1);
            }
            if ui.button("×").on_hover_text(format!("Remove {}", v.name)).clicked() {
                st.remove_var(side, &v.name);
            }
        });
    }
    if list.is_empty() {
        ui.label(RichText::new("None yet").color(pal.dim));
    }
    ui.horizontal(|ui| {
        let (buf, label) = match side {
            Side::Input => (&mut b.new_input, "Add input"),
            Side::Output => (&mut b.new_output, "Add output"),
        };
        let resp = ui.add(
            TextEdit::singleline(buf)
                .hint_text("Name, or Name[7..0]")
                .desired_width(150.0),
        );
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::accesskit::Role::TextInput,
                true,
                format!("New {title} variable"),
            )
        });
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button(label).clicked() || enter) && !buf.trim().is_empty() {
            let text = buf.clone();
            match st.add_var(side, &text) {
                Ok(()) => {
                    buf.clear();
                    b.error = None;
                }
                Err(e) => b.error = Some(e),
            }
        }
    });
}
