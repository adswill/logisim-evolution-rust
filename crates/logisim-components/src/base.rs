//! The Base library: the Text label. (Logisim's Select/Poke/Edit tools belong to the app.)
//!
//! Text is a pure annotation: no ports, no simulation.

use logisim_core::{Component, OptionMap, OptionSpec, Port, PropagateCtx};

use crate::registry::{Library, PartDesc};

pub fn library() -> Library {
    Library { name: "Base", parts: vec![PartDesc { name: "Text", make: || Box::new(Text::new()) }] }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HAlign {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    Top,
    /// Text sits on its baseline at the anchor (Logisim default).
    #[default]
    Base,
    Bottom,
    Center,
}

const HALIGNS: [(&str, HAlign); 3] = [("left", HAlign::Left), ("center", HAlign::Center), ("right", HAlign::Right)];
const VALIGNS: [(&str, VAlign); 4] = [("top", VAlign::Top), ("base", VAlign::Base), ("bottom", VAlign::Bottom), ("center", VAlign::Center)];

/// A text annotation (possibly several lines) placed on the canvas.
#[derive(Debug, Clone)]
pub struct Text {
    pub text: String,
    pub font_size: u8,
    pub bold: bool,
    pub halign: HAlign,
    pub valign: VAlign,
    /// Colour as `#rrggbb`.
    pub color: String,
}

impl Text {
    pub fn new() -> Self {
        Text { text: String::new(), font_size: 12, bold: true, halign: HAlign::Center, valign: VAlign::Base, color: "#000000".into() }
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

fn valid_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

impl Component for Text {
    fn name(&self) -> &str {
        "Text"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new()
            .with("text", self.text.as_str())
            .with("font_size", self.font_size)
            .with("bold", self.bold)
            .with("halign", OptionMap::name_of(self.halign, &HALIGNS))
            .with("valign", OptionMap::name_of(self.valign, &VALIGNS))
            .with("color", self.color.as_str())
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::string("text", "Text", ""),
            OptionSpec::int("font_size", "Font size", 4, 200, 12),
            OptionSpec::boolean("bold", "Bold", true),
            OptionSpec::choice("halign", "Horizontal align", &["left", "center", "right"], "center"),
            OptionSpec::choice("valign", "Vertical align", &["top", "base", "bottom", "center"], "base"),
            OptionSpec::string("color", "Colour (#rrggbb)", "#000000"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["text", "font_size", "bold", "halign", "valign", "color"])?;
        if let Some(t) = m.string("text")? {
            self.text = t.to_string();
        }
        if let Some(v) = m.int("font_size", 4, 200)? {
            self.font_size = v as u8;
        }
        self.bold = m.boolean("bold")?.unwrap_or(self.bold);
        self.halign = m.choice("halign", &HALIGNS)?.unwrap_or(self.halign);
        self.valign = m.choice("valign", &VALIGNS)?.unwrap_or(self.valign);
        if let Some(c) = m.string("color")? {
            if !valid_color(c) {
                return Err(format!("option 'color' = '{c}' must look like #rrggbb"));
            }
            self.color = c.to_ascii_lowercase();
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        Vec::new()
    }
    fn propagate(&mut self, _ctx: &mut dyn PropagateCtx) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_round_trip_and_validate() {
        let mut t = Text::new();
        let o = OptionMap::new()
            .with("text", "Adder\nstage 1")
            .with("font_size", 20)
            .with("bold", false)
            .with("halign", "left")
            .with("valign", "top")
            .with("color", "#FF8800");
        t.set_options(&o).unwrap();
        let mut u = Text::new();
        u.set_options(&t.options()).unwrap();
        assert_eq!(u.options(), t.options());
        assert_eq!(u.options().string("color").unwrap(), Some("#ff8800"));
        assert!(t.ports().is_empty());
        for bad in [OptionMap::new().with("color", "red"), OptionMap::new().with("font_size", 3), OptionMap::new().with("bogus", 1)] {
            assert!(t.set_options(&bad).is_err());
        }
    }

    #[test]
    fn library_specs_are_valid() {
        for p in &library().parts {
            logisim_core::validate_specs(&mut *(p.make)()).unwrap_or_else(|e| panic!("{}: {e}", p.name));
        }
    }
}
