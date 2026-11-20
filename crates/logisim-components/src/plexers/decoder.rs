//! Decoder: drives exactly one of `2^select_bits` 1-bit outputs high.
//!
//! Port order: `0..n` = outputs 0..n-1; `n` = select at (0,0); `n+1` = enable at (-10,0)
//! (only if `enable`). `n = 2^select_bits`. Outputs lie east of the select pin, as in Logisim.

use logisim_core::{BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{classify_enable, DisabledOutput, Enabled, SelectLoc, DELAY};

#[derive(Debug, Clone)]
pub struct DecoderOptions {
    /// 1..=8 select bits.
    pub select_bits: u8,
    pub enable: bool,
    pub disabled: DisabledOutput,
    pub select_loc: SelectLoc,
    /// Unselected outputs float instead of driving 0.
    pub tristate: bool,
}

impl Default for DecoderOptions {
    fn default() -> Self {
        DecoderOptions {
            select_bits: 1,
            enable: false,
            disabled: DisabledOutput::Zero,
            select_loc: SelectLoc::BottomLeft,
            tristate: false,
        }
    }
}

impl DecoderOptions {
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
    pub fn tristate(mut self, on: bool) -> Self {
        self.tristate = on;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Decoder {
    opts: DecoderOptions,
}

impl Decoder {
    pub fn new(opts: DecoderOptions) -> Self {
        Decoder { opts }
    }
    fn outputs(&self) -> usize {
        1 << self.opts.select_bits
    }
}

impl Component for Decoder {
    fn name(&self) -> &str {
        "Decoder"
    }

    fn ports(&self) -> Vec<Port> {
        let o = &self.opts;
        let n = self.outputs() as i32;
        let top_right = o.select_loc == SelectLoc::TopRight;
        let mut ps = Vec::new();
        if n == 2 {
            let ys = if top_right { [10, 30] } else { [-30, -10] };
            for y in ys {
                ps.push(Port::new(10, y, PortKind::Output, BitWidth::ONE));
            }
        } else {
            let y0 = if top_right { 0 } else { -10 * n };
            for i in 0..n {
                ps.push(Port::new(20, y0 + 10 * i, PortKind::Output, BitWidth::ONE));
            }
        }
        ps.push(Port::new(0, 0, PortKind::Input, BitWidth::new(o.select_bits)));
        if o.enable {
            ps.push(Port::new(-10, 0, PortKind::Input, BitWidth::ONE));
        }
        ps
    }

    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let o = &self.opts;
        let n = self.outputs();
        let en = if o.enable { ctx.port_value(n + 1) } else { Value::TRUE };
        let connected = o.enable && ctx.is_port_connected(n + 1);
        let mut others = if o.tristate { Value::UNKNOWN } else { Value::FALSE };
        let mut selected = None;
        match classify_enable(en, connected) {
            Enabled::Off => others = o.disabled.bit(),
            Enabled::Error => others = Value::ERROR,
            Enabled::On => {
                let sel = ctx.port_value(n);
                if sel.is_fully_defined() {
                    selected = Some(sel.to_u64().unwrap() as usize);
                } else if sel.is_error() {
                    others = Value::ERROR;
                } else {
                    others = Value::UNKNOWN;
                }
            }
        }
        for i in 0..n {
            ctx.set_port(i, if selected == Some(i) { Value::TRUE } else { others }, DELAY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plexers::testutil::Rig;

    #[test]
    fn two_to_four_one_hot() {
        let sel_w = BitWidth::new(2);
        for sel in 0..4u64 {
            let mut r = Rig::new(
                Box::new(Decoder::new(DecoderOptions::default().select_bits(2))),
                &[(4, Value::known(sel_w, sel))],
            );
            for i in 0..4 {
                let want = if i as u64 == sel { Value::TRUE } else { Value::FALSE };
                assert_eq!(r.get(i), want, "sel={sel} out={i}");
            }
        }
    }

    #[test]
    fn tristate_unselected_float() {
        let mut r = Rig::new(
            Box::new(Decoder::new(DecoderOptions::default().tristate(true))),
            &[(2, Value::TRUE)],
        );
        assert_eq!((r.get(0), r.get(1)), (Value::UNKNOWN, Value::TRUE));
    }

    #[test]
    fn enable_and_bad_selects() {
        let opts = DecoderOptions::default().enable(true);
        let mut r = Rig::new(Box::new(Decoder::new(opts)), &[(2, Value::TRUE), (3, Value::TRUE)]);
        assert_eq!((r.get(0), r.get(1)), (Value::FALSE, Value::TRUE));
        r.set(3, Value::FALSE);
        assert_eq!((r.get(0), r.get(1)), (Value::FALSE, Value::FALSE));
        r.set(3, Value::ERROR);
        assert_eq!((r.get(0), r.get(1)), (Value::ERROR, Value::ERROR));
        r.set(3, Value::TRUE);
        r.set(2, Value::UNKNOWN);
        assert_eq!((r.get(0), r.get(1)), (Value::UNKNOWN, Value::UNKNOWN));
        r.set(2, Value::ERROR);
        assert_eq!((r.get(0), r.get(1)), (Value::ERROR, Value::ERROR));
    }

    #[test]
    fn port_layout_east() {
        let p = |o: DecoderOptions| -> Vec<(i32, i32)> {
            Decoder::new(o).ports().iter().map(|p| (p.offset.x, p.offset.y)).collect()
        };
        assert_eq!(p(DecoderOptions::default()), [(10, -30), (10, -10), (0, 0)]);
        assert_eq!(
            p(DecoderOptions::default().select_loc(SelectLoc::TopRight).enable(true)),
            [(10, 10), (10, 30), (0, 0), (-10, 0)]
        );
        assert_eq!(
            p(DecoderOptions::default().select_bits(2)),
            [(20, -40), (20, -30), (20, -20), (20, -10), (0, 0)]
        );
    }
}
