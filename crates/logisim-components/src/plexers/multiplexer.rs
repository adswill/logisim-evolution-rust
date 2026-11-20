//! Multiplexer: routes one of `2^select_bits` data inputs to the output.
//!
//! Port order: 0 = output at (0,0); `1..=n` = data inputs 0..n-1; `n+1` = select;
//! `n+2` = enable (only if `enable` is set). `n = 2^select_bits`.

use logisim_core::{OptionMap, OptionSpec, BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{classify_enable, DisabledOutput, Enabled, SelectLoc, DELAY, DISABLED_NAMES, SELECT_LOC_NAMES};

const SIZES: [(&str, MuxSize); 2] = [("narrow", MuxSize::Narrow), ("wide", MuxSize::Wide)];

/// Body size of the multiplexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MuxSize {
    Narrow,
    /// Logisim default.
    #[default]
    Wide,
}

#[derive(Debug, Clone)]
pub struct MuxOptions {
    /// Data width in bits (1..=64).
    pub width: BitWidth,
    /// Number of select bits (1..=8); the mux has `2^select_bits` inputs.
    pub select_bits: u8,
    pub enable: bool,
    pub disabled: DisabledOutput,
    pub select_loc: SelectLoc,
    pub size: MuxSize,
}

impl Default for MuxOptions {
    fn default() -> Self {
        MuxOptions {
            width: BitWidth::ONE,
            select_bits: 1,
            enable: false,
            disabled: DisabledOutput::Zero,
            select_loc: SelectLoc::BottomLeft,
            size: MuxSize::Wide,
        }
    }
}

impl MuxOptions {
    pub fn width(mut self, bits: u8) -> Self {
        self.width = BitWidth::new(bits);
        self
    }
    pub fn select_bits(mut self, bits: u8) -> Self {
        assert!((1..=8).contains(&bits), "select bits must be 1..=8");
        self.select_bits = bits;
        self
    }
    pub fn enable(mut self, on: bool) -> Self {
        self.enable = on;
        self
    }
    pub fn disabled(mut self, d: DisabledOutput) -> Self {
        self.disabled = d;
        self
    }
    pub fn select_loc(mut self, l: SelectLoc) -> Self {
        self.select_loc = l;
        self
    }
    pub fn size(mut self, s: MuxSize) -> Self {
        self.size = s;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Multiplexer {
    opts: MuxOptions,
}

impl Multiplexer {
    pub fn new(opts: MuxOptions) -> Self {
        Multiplexer { opts }
    }
    fn inputs(&self) -> usize {
        1 << self.opts.select_bits
    }
}

impl Component for Multiplexer {
    fn name(&self) -> &str {
        "Multiplexer"
    }

    fn options(&self) -> OptionMap {
        let o = &self.opts;
        OptionMap::new()
            .with("width", o.width.get())
            .with("select_bits", o.select_bits)
            .with("enable", o.enable)
            .with("disabled", OptionMap::name_of(o.disabled, &DISABLED_NAMES))
            .with("select_loc", OptionMap::name_of(o.select_loc, &SELECT_LOC_NAMES))
            .with("size", OptionMap::name_of(o.size, &SIZES))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("width", "Data bits", 1, 64, 1),
            OptionSpec::int("select_bits", "Select bits", 1, 8, 1),
            OptionSpec::boolean("enable", "Enable input", false),
            OptionSpec::choice("disabled", "Disabled output", &["zero", "floating"], "zero"),
            OptionSpec::choice("select_loc", "Select location", &["bottom_left", "top_right"], "bottom_left"),
            OptionSpec::choice("size", "Size", &["narrow", "wide"], "wide"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width", "select_bits", "enable", "disabled", "select_loc", "size"])?;
        let o = &mut self.opts;
        if let Some(v) = m.int("width", 1, 64)? {
            o.width = BitWidth::new(v as u8);
        }
        if let Some(v) = m.int("select_bits", 1, 8)? {
            o.select_bits = v as u8;
        }
        o.enable = m.boolean("enable")?.unwrap_or(o.enable);
        o.disabled = m.choice("disabled", &DISABLED_NAMES)?.unwrap_or(o.disabled);
        o.select_loc = m.choice("select_loc", &SELECT_LOC_NAMES)?.unwrap_or(o.select_loc);
        o.size = m.choice("size", &SIZES)?.unwrap_or(o.size);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let o = &self.opts;
        let n = self.inputs();
        let wide = o.size == MuxSize::Wide;
        let mult = o.select_loc.mult();
        let mut ps = vec![Port::new(0, 0, PortKind::Output, o.width)];
        let (mut sx, sy);
        if n == 2 {
            let w = if wide { 30 } else { 20 };
            sx = -(if wide { 20 } else { 10 });
            sy = mult * 20;
            ps.push(Port::new(-w, -10, PortKind::Input, o.width));
            ps.push(Port::new(-w, 10, PortKind::Input, o.width));
        } else {
            let w = if wide { 40 } else { 20 };
            let y0 = -(n as i32 / 2) * 10;
            sx = -(if wide { 20 } else { 10 });
            sy = mult * (y0 + 10 * n as i32);
            for i in 0..n as i32 {
                ps.push(Port::new(-w, y0 + 10 * i, PortKind::Input, o.width));
            }
            // Narrow, select on the bottom-left: nudge the select pin left (Logisim quirk).
            if !wide && o.select_loc == SelectLoc::BottomLeft {
                sx -= 10;
            }
        }
        ps.push(Port::new(sx, sy, PortKind::Input, BitWidth::new(o.select_bits)));
        if o.enable {
            ps.push(Port::new(sx + 10, sy, PortKind::Input, BitWidth::ONE));
        }
        ps
    }

    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let o = &self.opts;
        let n = self.inputs();
        let en_port = n + 2;
        let en = if o.enable { ctx.port_value(en_port) } else { Value::TRUE };
        let connected = o.enable && ctx.is_port_connected(en_port);
        let out = match classify_enable(en, connected) {
            Enabled::Off => Value::repeat(o.disabled.bit(), o.width),
            Enabled::Error => Value::error(o.width),
            Enabled::On => {
                let sel = ctx.port_value(n + 1);
                if sel.is_fully_defined() {
                    ctx.port_value(1 + sel.to_u64().unwrap() as usize)
                } else if sel.is_error() {
                    Value::error(o.width)
                } else {
                    Value::unknown(o.width)
                }
            }
        };
        ctx.set_port(0, out, DELAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plexers::testutil::Rig;

    fn bit(b: bool) -> Value {
        if b { Value::TRUE } else { Value::FALSE }
    }

    #[test]
    fn two_input_truth_table() {
        for (a, b, s) in (0..8).map(|i| (i & 4 != 0, i & 2 != 0, i & 1 != 0)) {
            let mut r = Rig::new(
                Box::new(Multiplexer::new(MuxOptions::default())),
                &[(1, bit(a)), (2, bit(b)), (3, bit(s))],
            );
            assert_eq!(r.get(0), bit(if s { b } else { a }), "a={a} b={b} s={s}");
        }
    }

    #[test]
    fn four_input_wide_bus_selects_each_input() {
        let opts = MuxOptions::default().select_bits(2).width(4);
        let data = [0x3u64, 0x5, 0xA, 0xC];
        for sel in 0..4u64 {
            let mut ins: Vec<(usize, Value)> = data
                .iter()
                .enumerate()
                .map(|(i, &d)| (1 + i, Value::known(BitWidth::new(4), d)))
                .collect();
            ins.push((5, Value::known(BitWidth::new(2), sel)));
            let mut r = Rig::new(Box::new(Multiplexer::new(opts.clone())), &ins);
            assert_eq!(r.get(0), Value::known(BitWidth::new(4), data[sel as usize]));
        }
    }

    #[test]
    fn unknown_and_error_select() {
        let mut r = Rig::new(
            Box::new(Multiplexer::new(MuxOptions::default().width(2))),
            &[
                (1, Value::known(BitWidth::new(2), 1)),
                (2, Value::known(BitWidth::new(2), 2)),
                (3, Value::UNKNOWN),
            ],
        );
        assert_eq!(r.get(0), Value::unknown(BitWidth::new(2)));
        r.set(3, Value::ERROR);
        assert_eq!(r.get(0), Value::error(BitWidth::new(2)));
        r.set(3, Value::TRUE);
        assert_eq!(r.get(0), Value::known(BitWidth::new(2), 2));
    }

    #[test]
    fn enable_and_disabled_output() {
        for (mode, want) in [(DisabledOutput::Zero, Value::FALSE), (DisabledOutput::Floating, Value::UNKNOWN)] {
            let opts = MuxOptions::default().enable(true).disabled(mode);
            let mut r = Rig::new(
                Box::new(Multiplexer::new(opts)),
                &[(1, Value::TRUE), (2, Value::TRUE), (3, Value::FALSE), (4, Value::TRUE)],
            );
            assert_eq!(r.get(0), Value::TRUE);
            r.set(4, Value::FALSE);
            assert_eq!(r.get(0), want);
            r.set(4, Value::ERROR);
            assert_eq!(r.get(0), Value::ERROR);
        }
    }

    #[test]
    fn port_layout_east() {
        let m = Multiplexer::new(MuxOptions::default());
        let p: Vec<_> = m.ports().iter().map(|p| (p.offset.x, p.offset.y)).collect();
        assert_eq!(p, [(0, 0), (-30, -10), (-30, 10), (-20, 20)]);
        let m = Multiplexer::new(MuxOptions::default().select_bits(2).enable(true));
        let p: Vec<_> = m.ports().iter().map(|p| (p.offset.x, p.offset.y)).collect();
        // inputs at x=-40, y=-20..10; select below the body; enable one step toward the output
        assert_eq!(
            p,
            [(0, 0), (-40, -20), (-40, -10), (-40, 0), (-40, 10), (-20, 20), (-10, 20)]
        );
    }
}
