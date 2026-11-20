//! Priority encoder: outputs the index of the highest-numbered input that is high.
//!
//! Port order: 0 = encoded output (select_bits wide) at (0,0); `1..=n` = inputs 0..n-1;
//! `n+1` = enable in; `n+2` = enable out (high when enabled and no input is high);
//! `n+3` = group select (high when any input is high). `n = 2^select_bits`.

use logisim_core::{OptionMap, OptionSpec, BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{DisabledOutput, DELAY, DISABLED_NAMES};

#[derive(Debug, Clone)]
pub struct PriorityEncoderOptions {
    /// 1..=8 select bits (the part has `2^select_bits` inputs).
    pub select_bits: u8,
    pub disabled: DisabledOutput,
}

impl Default for PriorityEncoderOptions {
    fn default() -> Self {
        PriorityEncoderOptions { select_bits: 3, disabled: DisabledOutput::Zero }
    }
}

impl PriorityEncoderOptions {
    pub fn select_bits(mut self, bits: u8) -> Self {
        assert!((1..=8).contains(&bits), "select bits must be 1..=8");
        self.select_bits = bits;
        self
    }
    pub fn disabled(mut self, d: DisabledOutput) -> Self {
        self.disabled = d;
        self
    }
}

#[derive(Debug, Clone)]
pub struct PriorityEncoder {
    opts: PriorityEncoderOptions,
}

impl PriorityEncoder {
    pub fn new(opts: PriorityEncoderOptions) -> Self {
        PriorityEncoder { opts }
    }
    fn inputs(&self) -> usize {
        1 << self.opts.select_bits
    }
}

impl Component for PriorityEncoder {
    fn name(&self) -> &str {
        "Priority Encoder"
    }

    fn options(&self) -> OptionMap {
        OptionMap::new()
            .with("select_bits", self.opts.select_bits)
            .with("disabled", OptionMap::name_of(self.opts.disabled, &DISABLED_NAMES))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("select_bits", "Select bits", 1, 8, 3),
            OptionSpec::choice("disabled", "Disabled output", &["zero", "floating"], "zero"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["select_bits", "disabled"])?;
        if let Some(v) = m.int("select_bits", 1, 8)? {
            self.opts.select_bits = v as u8;
        }
        self.opts.disabled = m.choice("disabled", &DISABLED_NAMES)?.unwrap_or(self.opts.disabled);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let n = self.inputs() as i32;
        let sel = BitWidth::new(self.opts.select_bits);
        let (x, y) = (-40, -5 * n + 10);
        let mut ps = vec![Port::new(0, 0, PortKind::Output, sel)];
        for i in 0..n {
            ps.push(Port::new(x, y + 10 * i, PortKind::Input, BitWidth::ONE));
        }
        ps.push(Port::new(x / 2, y + 10 * n, PortKind::Input, BitWidth::ONE));
        ps.push(Port::new(x / 2, y - 10, PortKind::Output, BitWidth::ONE));
        ps.push(Port::new(0, 10, PortKind::Output, BitWidth::ONE));
        ps
    }

    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let n = self.inputs();
        let sel = BitWidth::new(self.opts.select_bits);
        // Logisim treats anything but a clean low as enabled (an unconnected enable floats high).
        let enabled = ctx.port_value(n + 1) != Value::FALSE;
        let mut hit = None;
        let default;
        if enabled {
            default = Value::unknown(sel);
            hit = (0..n).rev().find(|&i| ctx.port_value(1 + i) == Value::TRUE);
        } else {
            default = Value::repeat(self.opts.disabled.bit(), sel);
        }
        let bit = |b: bool| if b { Value::TRUE } else { Value::FALSE };
        match hit {
            None => {
                ctx.set_port(0, default, DELAY);
                ctx.set_port(n + 2, bit(enabled), DELAY);
                ctx.set_port(n + 3, Value::FALSE, DELAY);
            }
            Some(i) => {
                ctx.set_port(0, Value::known(sel, i as u64), DELAY);
                ctx.set_port(n + 2, Value::FALSE, DELAY);
                ctx.set_port(n + 3, Value::TRUE, DELAY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plexers::testutil::Rig;

    fn enc(bits: u8, mask: u32, en: Option<Value>, mode: DisabledOutput) -> Rig {
        let n = 1usize << bits;
        let mut ins: Vec<(usize, Value)> = (0..n)
            .map(|i| (1 + i, if mask >> i & 1 == 1 { Value::TRUE } else { Value::FALSE }))
            .collect();
        if let Some(e) = en {
            ins.push((n + 1, e));
        }
        let opts = PriorityEncoderOptions::default().select_bits(bits).disabled(mode);
        Rig::new(Box::new(PriorityEncoder::new(opts)), &ins)
    }

    #[test]
    fn highest_input_wins_exhaustive_4_inputs() {
        let w = BitWidth::new(2);
        for mask in 0..16u32 {
            let mut r = enc(2, mask, Some(Value::TRUE), DisabledOutput::Zero);
            if mask == 0 {
                assert_eq!(r.get(0), Value::unknown(w));
                assert_eq!((r.get(6), r.get(7)), (Value::TRUE, Value::FALSE));
            } else {
                let top = 31 - mask.leading_zeros() as u64;
                assert_eq!(r.get(0), Value::known(w, top), "mask={mask:04b}");
                assert_eq!((r.get(6), r.get(7)), (Value::FALSE, Value::TRUE));
            }
        }
    }

    #[test]
    fn disabled_outputs() {
        let w = BitWidth::new(3);
        let mut r = enc(3, 0b101, Some(Value::FALSE), DisabledOutput::Zero);
        assert_eq!(r.get(0), Value::known(w, 0));
        assert_eq!((r.get(10), r.get(11)), (Value::FALSE, Value::FALSE));
        let mut r = enc(3, 0b101, Some(Value::FALSE), DisabledOutput::Floating);
        assert_eq!(r.get(0), Value::unknown(w));
    }

    #[test]
    fn unconnected_enable_counts_as_enabled() {
        let mut r = enc(1, 0b01, None, DisabledOutput::Zero);
        assert_eq!(r.get(0), Value::known(BitWidth::new(1), 0));
        assert_eq!(r.get(5), Value::TRUE); // group select
    }

    #[test]
    fn port_layout_east() {
        let e = PriorityEncoder::new(PriorityEncoderOptions::default().select_bits(1));
        let p: Vec<_> = e.ports().iter().map(|p| (p.offset.x, p.offset.y)).collect();
        assert_eq!(p, [(0, 0), (-40, 0), (-40, 10), (-20, 20), (-20, -10), (0, 10)]);
    }
}
