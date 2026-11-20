//! Bit selector: outputs one `group`-bit slice of the data input, chosen by the select input.
//!
//! Port order: 0 = output (`group` bits) at (0,0); 1 = data input (-30,0); 2 = select (-10,±10).
//! The select width is derived from the data width and group size (see [`BitSelectorOptions::select_bits`]).

use logisim_core::{OptionMap, OptionSpec, BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{SelectLoc, DELAY, SELECT_LOC_NAMES};

#[derive(Debug, Clone)]
pub struct BitSelectorOptions {
    /// Data input width (Logisim default 8).
    pub width: BitWidth,
    /// Output slice width (default 1).
    pub group: BitWidth,
    pub select_loc: SelectLoc,
}

impl Default for BitSelectorOptions {
    fn default() -> Self {
        BitSelectorOptions { width: BitWidth::new(8), group: BitWidth::ONE, select_loc: SelectLoc::BottomLeft }
    }
}

impl BitSelectorOptions {
    pub fn width(mut self, bits: u8) -> Self {
        self.width = BitWidth::new(bits);
        self
    }
    pub fn group(mut self, bits: u8) -> Self {
        self.group = BitWidth::new(bits);
        self
    }
    pub fn select_loc(mut self, l: SelectLoc) -> Self {
        self.select_loc = l;
        self
    }
    /// Number of select bits needed to address every group (never less than 1).
    pub fn select_bits(&self) -> u8 {
        let g = self.group.get() as u32;
        let mut groups = (self.width.get() as u32).div_ceil(g) - 1;
        let mut bits = 1;
        if groups > 0 {
            while groups != 1 {
                groups >>= 1;
                bits += 1;
            }
        }
        bits
    }
}

#[derive(Debug, Clone)]
pub struct BitSelector {
    opts: BitSelectorOptions,
}

impl BitSelector {
    pub fn new(opts: BitSelectorOptions) -> Self {
        BitSelector { opts }
    }
}

impl Component for BitSelector {
    fn name(&self) -> &str {
        "Bit Selector"
    }

    fn options(&self) -> OptionMap {
        let o = &self.opts;
        OptionMap::new()
            .with("width", o.width.get())
            .with("group", o.group.get())
            .with("select_loc", OptionMap::name_of(o.select_loc, &SELECT_LOC_NAMES))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("width", "Data bits", 1, 64, 8),
            OptionSpec::int("group", "Output bits", 1, 64, 1),
            OptionSpec::choice("select_loc", "Select location", &["bottom_left", "top_right"], "bottom_left"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width", "group", "select_loc"])?;
        let o = &mut self.opts;
        if let Some(v) = m.int("width", 1, 64)? {
            o.width = BitWidth::new(v as u8);
        }
        if let Some(v) = m.int("group", 1, 64)? {
            o.group = BitWidth::new(v as u8);
        }
        o.select_loc = m.choice("select_loc", &SELECT_LOC_NAMES)?.unwrap_or(o.select_loc);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let o = &self.opts;
        let sel_y = if o.select_loc == SelectLoc::BottomLeft { 10 } else { -10 };
        vec![
            Port::new(0, 0, PortKind::Output, o.group),
            Port::new(-30, 0, PortKind::Input, o.width),
            Port::new(-10, sel_y, PortKind::Input, BitWidth::new(self.opts.select_bits())),
        ]
    }

    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let group = self.opts.group;
        let data = ctx.port_value(1);
        let select = ctx.port_value(2);
        let out = if !select.is_fully_defined() {
            Value::unknown(group)
        } else {
            let shift = select.to_u64().unwrap() as usize * group.get() as usize;
            let dw = data.width().get() as usize;
            if shift >= dw {
                Value::known(group, 0)
            } else {
                let bits: Vec<Value> = (0..group.get() as usize)
                    .map(|i| if shift + i >= dw { Value::FALSE } else { data.bit((shift + i) as u8) })
                    .collect();
                Value::from_bits(&bits)
            }
        };
        ctx.set_port(0, out, DELAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plexers::testutil::Rig;

    #[test]
    fn select_bits_derivation() {
        let bits = |w, g| BitSelectorOptions::default().width(w).group(g).select_bits();
        assert_eq!(bits(8, 1), 3);
        assert_eq!(bits(2, 1), 1);
        assert_eq!(bits(1, 1), 1);
        assert_eq!(bits(9, 1), 4);
        assert_eq!(bits(8, 4), 1);
        assert_eq!(bits(12, 4), 2);
    }

    #[test]
    fn single_bit_every_index() {
        let data = 0b1011_0010u64;
        for sel in 0..8u64 {
            let mut r = Rig::new(
                Box::new(BitSelector::new(BitSelectorOptions::default())),
                &[(1, Value::known(BitWidth::new(8), data)), (2, Value::known(BitWidth::new(3), sel))],
            );
            let want = if data >> sel & 1 == 1 { Value::TRUE } else { Value::FALSE };
            assert_eq!(r.get(0), want, "sel={sel}");
        }
    }

    #[test]
    fn grouped_slices_and_overrun() {
        let opts = BitSelectorOptions::default().width(10).group(4); // 3 groups, 2 select bits
        let w4 = BitWidth::new(4);
        let data = Value::known(BitWidth::new(10), 0b10_0110_1001);
        let want = [0b1001u64, 0b0110, 0b0010, 0];
        for (sel, want) in want.into_iter().enumerate() {
            let mut r = Rig::new(
                Box::new(BitSelector::new(opts.clone())),
                &[(1, data), (2, Value::known(BitWidth::new(2), sel as u64))],
            );
            assert_eq!(r.get(0), Value::known(w4, want), "sel={sel}");
        }
    }

    #[test]
    fn unknown_select_gives_unknown() {
        let mut r = Rig::new(
            Box::new(BitSelector::new(BitSelectorOptions::default())),
            &[(1, Value::known(BitWidth::new(8), 0xFF)), (2, Value::unknown(BitWidth::new(3)))],
        );
        assert_eq!(r.get(0), Value::UNKNOWN);
    }

    #[test]
    fn port_layout_east() {
        let p = |l| -> Vec<(i32, i32)> {
            BitSelector::new(BitSelectorOptions::default().select_loc(l))
                .ports()
                .iter()
                .map(|p| (p.offset.x, p.offset.y))
                .collect()
        };
        assert_eq!(p(SelectLoc::BottomLeft), [(0, 0), (-30, 0), (-10, 10)]);
        assert_eq!(p(SelectLoc::TopRight), [(0, 0), (-30, 0), (-10, -10)]);
    }
}
