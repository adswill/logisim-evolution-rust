//! RAM/ROM contents editor.
//!
//! Entry points:
//! * [`HexEditor::for_component`] reads `addr_bits`, `data_bits` and `contents` from a
//!   `ComponentDef` (ROM or RAM);
//! * [`HexEditor::show`] draws the window and returns [`HexEvent::Apply`] when the user presses
//!   "Apply";
//! * [`HexEditor::apply_to`] writes the edited contents back into a `ComponentDef`.
//!
//! The editor also imports/exports Logisim's `v2.0 raw` hex files, so images from the Java
//! version load unchanged.

use eframe::egui::{self, Color32, RichText, TextEdit};
use logisim_components::memory::MemContents;
use logisim_format::ComponentDef;

const HEADER: &str = "v2.0 raw";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HexEvent {
    None,
    /// The user wants the edited contents stored in the component.
    Apply,
}

pub struct HexEditor {
    title: String,
    mem: MemContents,
    /// Words per row.
    cols: u64,
    /// The cell being edited: address and its text.
    editing: Option<(u64, String)>,
    goto: String,
    fill_from: String,
    fill_to: String,
    fill_val: String,
    message: Option<(bool, String)>,
    scroll_to: Option<u64>,
    changed: bool,
}

/// Parse one hexadecimal word, tolerating `0x` and `_`.
pub fn parse_hex(s: &str) -> Option<u64> {
    let t = s
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .replace('_', "");
    u64::from_str_radix(&t, 16).ok()
}

/// Parse text in Logisim's raw format (also plain hex words): optional `v2.0 raw` header,
/// `#` comments, words separated by spaces/newlines, `N*V` meaning `V` repeated `N` times
/// (N decimal, V hex).
pub fn load_raw(mem: &mut MemContents, text: &str) -> Result<(), String> {
    let mut addr = 0u64;
    let limit = mem.last_offset() + 1;
    for (ln, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() || (ln == 0 && line.eq_ignore_ascii_case(HEADER)) {
            continue;
        }
        for tok in line.split_whitespace() {
            let (n, v) = match tok.split_once('*') {
                Some((n, v)) => (
                    n.parse::<u64>()
                        .map_err(|_| format!("Line {}: `{n}` is not a repeat count.", ln + 1))?,
                    v,
                ),
                None => (1, tok),
            };
            let v = parse_hex(v)
                .ok_or_else(|| format!("Line {}: `{v}` is not a hexadecimal word.", ln + 1))?;
            if mem.width() < 64 && v >> mem.width() != 0 {
                return Err(format!(
                    "Line {}: `{tok}` does not fit in {} bits.",
                    ln + 1,
                    mem.width()
                ));
            }
            if addr.saturating_add(n) > limit {
                return Err(format!(
                    "The file has more than the {limit} words this memory holds (stopped at line {}).",
                    ln + 1
                ));
            }
            mem.fill(addr, n, v);
            addr += n;
        }
    }
    Ok(())
}

/// Logisim `v2.0 raw` text for the used part of the memory (8 words per line, runs compressed).
pub fn save_raw(mem: &MemContents) -> String {
    let digits = (mem.width() as usize).div_ceil(4);
    let len = mem.used_len();
    let mut out = format!("{HEADER}\n");
    let mut words: Vec<String> = vec![];
    let mut a = 0;
    while a < len {
        let v = mem.get(a);
        let mut n = 1;
        while a + n < len && mem.get(a + n) == v {
            n += 1;
        }
        if n >= 4 {
            words.push(format!("{n}*{v:0digits$x}"));
            a += n;
        } else {
            words.push(format!("{v:0digits$x}"));
            a += 1;
        }
    }
    for chunk in words.chunks(8) {
        out += &chunk.join(" ");
        out.push('\n');
    }
    out
}

impl HexEditor {
    pub fn new(title: &str, addr_bits: u8, data_bits: u8) -> Self {
        let mem = MemContents::new(addr_bits.clamp(1, 32), data_bits.clamp(1, 64));
        let cols = if mem.width() <= 8 {
            16
        } else if mem.width() <= 16 {
            8
        } else {
            4
        };
        HexEditor {
            title: title.to_string(),
            mem,
            cols,
            editing: None,
            goto: String::new(),
            fill_from: String::new(),
            fill_to: String::new(),
            fill_val: "0".into(),
            message: None,
            scroll_to: None,
            changed: false,
        }
    }

    /// Open the contents of a ROM/RAM component definition.
    pub fn for_component(def: &ComponentDef) -> Result<Self, String> {
        let bits = |k: &str, d: i64| def.options.int(k, 1, 64).ok().flatten().unwrap_or(d) as u8;
        let mut ed = HexEditor::new(
            &format!("{} contents", def.part),
            bits("addr_bits", 8),
            bits("data_bits", 8),
        );
        if let Ok(Some(t)) = def.options.string("contents") {
            ed.mem
                .load_text(t)
                .map_err(|e| format!("The stored contents are invalid: {e}"))?;
        }
        Ok(ed)
    }

    /// Store the edited contents in `def`'s `contents` option.
    pub fn apply_to(&self, def: &mut ComponentDef) {
        def.options.set("contents", self.mem.to_text());
    }

    pub fn contents(&self) -> &MemContents {
        &self.mem
    }

    /// True once something was edited since opening or the last [`HexEditor::mark_applied`].
    pub fn is_modified(&self) -> bool {
        self.changed
    }

    pub fn mark_applied(&mut self) {
        self.changed = false;
    }

    pub fn load_text(&mut self, text: &str) -> Result<(), String> {
        let mut fresh = MemContents::new(self.mem.addr_bits(), self.mem.width());
        load_raw(&mut fresh, text)?;
        self.mem = fresh;
        self.changed = true;
        Ok(())
    }

    pub fn to_raw_text(&self) -> String {
        save_raw(&self.mem)
    }

    fn digits(&self) -> usize {
        (self.mem.width() as usize).div_ceil(4)
    }

    fn addr_digits(&self) -> usize {
        (self.mem.addr_bits() as usize).div_ceil(4)
    }

    /// Show the window. `open` is cleared when closed. Draw it every frame while open.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) -> HexEvent {
        let mut ev = HexEvent::None;
        let title = format!("{}{}", self.title, if self.changed { " •" } else { "" });
        egui::Window::new(title)
            .id(egui::Id::unique(("hexedit", self.title.clone())))
            .open(open)
            .default_size([620.0, 460.0])
            .show(ctx, |ui| ev = self.ui(ui));
        ev
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> HexEvent {
        let mut ev = HexEvent::None;
        let last = self.mem.last_offset();
        ui.horizontal_wrapped(|ui| {
            ui.label("Go to (hex):");
            let r = ui.add(TextEdit::singleline(&mut self.goto).desired_width(80.0));
            if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                || ui.button("Go").clicked()
            {
                match parse_hex(&self.goto) {
                    Some(a) if a <= last => self.scroll_to = Some(a),
                    _ => self.message = Some((true, format!("Addresses run from 0 to {last:x}."))),
                }
            }
            ui.separator();
            if ui.button("Open file…").clicked() {
                self.open_file();
            }
            if ui.button("Save file…").clicked() {
                self.save_file();
            }
            if ui.button("Clear all").clicked() {
                self.mem.clear();
                self.changed = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Fill from");
            ui.add(
                TextEdit::singleline(&mut self.fill_from)
                    .desired_width(70.0)
                    .hint_text("0"),
            );
            ui.label("to");
            ui.add(
                TextEdit::singleline(&mut self.fill_to)
                    .desired_width(70.0)
                    .hint_text(format!("{last:x}")),
            );
            ui.label("with");
            ui.add(TextEdit::singleline(&mut self.fill_val).desired_width(70.0));
            if ui.button("Fill").clicked() {
                self.do_fill();
            }
        });
        ui.separator();

        let mono = egui::TextStyle::Monospace.resolve(ui.style());
        let row_h = ui.fonts_mut(|f| f.row_height(&mono)) + 4.0;
        let rows = (last / self.cols + 1).min(usize::MAX as u64) as usize;
        let mut area = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(ui.available_height() - 60.0);
        if let Some(a) = self.scroll_to.take() {
            area = area.vertical_scroll_offset((a / self.cols) as f32 * row_h);
            self.editing = Some((a, format!("{:0w$x}", self.mem.get(a), w = self.digits())));
        }
        let (digits, adigits, cols, width) = (
            self.digits(),
            self.addr_digits(),
            self.cols,
            self.mem.width(),
        );
        area.show_rows(ui, row_h, rows, |ui, range| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for row in range {
                let base = row as u64 * cols;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{base:0adigits$x}:"))
                            .monospace()
                            .color(Color32::GRAY),
                    );
                    for c in 0..cols {
                        let a = base + c;
                        if a > last {
                            break;
                        }
                        self.cell(ui, a, digits);
                    }
                    if width == 8 {
                        let s: String = (0..cols)
                            .filter(|c| base + c <= last)
                            .map(|c| {
                                let b = self.mem.get(base + c) as u8;
                                if (0x20..0x7f).contains(&b) {
                                    b as char
                                } else {
                                    '.'
                                }
                            })
                            .collect();
                        ui.label(RichText::new(s).monospace().color(Color32::GRAY));
                    }
                });
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Apply to component").clicked() {
                ev = HexEvent::Apply;
            }
            ui.weak(format!(
                "{} words × {} bits · values in hex",
                last + 1,
                self.mem.width()
            ));
        });
        if let Some((err, m)) = &self.message {
            let c = if *err {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            };
            ui.colored_label(c, m);
        }
        ev
    }

    fn cell(&mut self, ui: &mut egui::Ui, a: u64, digits: usize) {
        let editing_here = matches!(&self.editing, Some((ea, _)) if *ea == a);
        if editing_here {
            let (_, text) = self.editing.as_mut().unwrap();
            let r = ui.add(
                TextEdit::singleline(text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(8.0 * digits as f32 + 10.0),
            );
            r.request_focus();
            let enter =
                ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Tab));
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if esc {
                self.editing = None;
            } else if enter || r.lost_focus() {
                let (_, text) = self.editing.take().unwrap();
                match parse_hex(&text) {
                    Some(v) if self.mem.width() >= 64 || v >> self.mem.width() == 0 => {
                        self.mem.set(a, v);
                        self.changed = true;
                        self.message = None;
                        if enter && a < self.mem.last_offset() {
                            self.editing =
                                Some((a + 1, format!("{:0digits$x}", self.mem.get(a + 1))));
                        }
                    }
                    _ => {
                        self.message = Some((
                            true,
                            format!(
                                "`{}` is not a hex value that fits in {} bits.",
                                text.trim(),
                                self.mem.width()
                            ),
                        ));
                    }
                }
            }
        } else {
            let v = self.mem.get(a);
            let mut t = RichText::new(format!("{v:0digits$x}")).monospace();
            if v == 0 {
                t = t.color(Color32::GRAY);
            }
            if ui
                .add(egui::Label::new(t).sense(egui::Sense::click()))
                .clicked()
            {
                self.editing = Some((a, format!("{v:0digits$x}")));
            }
        }
    }

    fn do_fill(&mut self) {
        let last = self.mem.last_offset();
        let from = if self.fill_from.trim().is_empty() {
            Some(0)
        } else {
            parse_hex(&self.fill_from)
        };
        let to = if self.fill_to.trim().is_empty() {
            Some(last)
        } else {
            parse_hex(&self.fill_to)
        };
        let val = parse_hex(&self.fill_val);
        self.message = match (from, to, val) {
            (Some(f), Some(t), Some(v))
                if f <= t
                    && t <= last
                    && (self.mem.width() >= 64 || v >> self.mem.width() == 0) =>
            {
                self.mem.fill(f, t - f + 1, v);
                self.changed = true;
                None
            }
            (Some(f), Some(t), _) if f > t || t > last => Some((
                true,
                format!("Give a range inside 0 to {last:x}, start first."),
            )),
            _ => Some((
                true,
                format!(
                    "Enter hex numbers; the value must fit in {} bits.",
                    self.mem.width()
                ),
            )),
        };
    }

    fn open_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Memory image", &["hex", "txt", "mem"])
            .add_filter("All files", &["*"])
            .pick_file()
        else {
            return;
        };
        self.message = match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| self.load_text(&t))
        {
            Ok(()) => Some((false, format!("Loaded {}.", path.display()))),
            Err(e) => Some((true, e)),
        };
    }

    fn save_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("memory.hex")
            .save_file()
        else {
            return;
        };
        self.message = match std::fs::write(&path, self.to_raw_text()) {
            Ok(()) => Some((false, format!("Saved {}.", path.display()))),
            Err(e) => Some((true, format!("Could not save: {e}"))),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_round_trip_with_runs_and_header() {
        let mut m = MemContents::new(8, 8);
        m.fill(0, 10, 0xab);
        m.set(10, 0x01);
        m.set(11, 0xff);
        let text = save_raw(&m);
        assert!(text.starts_with("v2.0 raw\n10*ab 01 ff"), "{text}");
        let mut n = MemContents::new(8, 8);
        load_raw(&mut n, &text).unwrap();
        assert_eq!(n.get(9), 0xab);
        assert_eq!(n.get(10), 0x01);
        assert_eq!(n.get(11), 0xff);
        assert_eq!(n.get(12), 0);
    }

    #[test]
    fn loads_plain_hex_and_comments() {
        let mut m = MemContents::new(4, 8);
        load_raw(&mut m, "0x10 20 # comment\n30\n").unwrap();
        assert_eq!((m.get(0), m.get(1), m.get(2)), (0x10, 0x20, 0x30));
    }

    #[test]
    fn bad_files_give_clear_errors() {
        let mut m = MemContents::new(2, 4);
        assert!(load_raw(&mut m, "zz")
            .unwrap_err()
            .contains("not a hexadecimal"));
        assert!(load_raw(&mut m, "1f")
            .unwrap_err()
            .contains("does not fit in 4 bits"));
        assert!(load_raw(&mut m, "5*1")
            .unwrap_err()
            .contains("more than the 4 words"));
    }

    #[test]
    fn component_round_trip() {
        let mut def = ComponentDef::new("Memory", "ROM", 0, 0).unwrap();
        def.options.set("addr_bits", 4);
        def.options.set("data_bits", 8);
        let mut ed = HexEditor::for_component(&def).unwrap();
        ed.load_text("12 34 5*ff").unwrap();
        assert!(ed.is_modified());
        ed.apply_to(&mut def);
        let again = HexEditor::for_component(&def).unwrap();
        assert_eq!(again.contents().get(1), 0x34);
        assert_eq!(again.contents().get(6), 0xff);
        assert_eq!(again.contents().get(7), 0);
        assert!(def.build().is_ok());
    }
}
