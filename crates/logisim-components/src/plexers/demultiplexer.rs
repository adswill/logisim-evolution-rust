//! Demultiplexer: routes the data input to one of `2^select_bits` outputs; others get a default.
//!
//! Port order: `0..n` = outputs 0..n-1 (east side, x = +30/+40); `n` = select; `n+1` = enable
//! (only if `enable`); last = data input at (0,0). `n = 2^select_bits`.
//! Unlike most parts the data pin is the origin, as in Logisim (outputs lie east of it).

use logisim_core::{BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{classify_enable, DisabledOutput, Enabled, SelectLoc, DELAY};

#[derive(Debug, Clone)]
pub struct DemuxOptions {
    pub width: BitWidth,
    /// 1..=8 select bits.
    pub select_bits: u8,
    pub enable: bool,
    pub disabled: DisabledOutput,
    pub select_loc: SelectLoc,
    /// Unselected outputs float instead of driving 0.
    pub tristate: bool,
}

impl Default for DemuxOptions {
    fn default() -> Self {
        DemuxOptions {
            width: BitWidth::ONE,
            select_bits: 1,
            enable: false,
            disabled: DisabledOutput::Zero,
            select_loc: SelectLoc::BottomLeft,
            tristate: false,
        }
    }
}

impl DemuxOptions {
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
    pub fn tristate(mut self, on: bool) -> Self {
        self.tristate = on;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Demultiplexer {
    opts: DemuxOptions,
}

impl Demultiplexer {
    pub fn new(opts: DemuxOptions) -> Self {
        Demultiplexer { opts }
    }
    fn outputs(&self) -> usize {
        1 << self.opts.select_bits
    }
}

impl Component for Demultiplexer {
    fn name(&self) -> &str {
        "Demultiplexer"
    }

    fn ports(&self) -> Vec<Port> {
        let o = &self.opts;
        let n = self.outputs() as i32;
        let mult = o.select_loc.mult();
        let mut ps = Vec::new();
        let (sx, sy);
        if n == 2 {
            ps.push(Port::new(30, -10, PortKind::Output, o.width));
            ps.push(Port::new(30, 10, PortKind::Output, o.width));
            sx = 20;
            sy = mult * 20;
        } else {
            let y0 = -(n / 2) * 10;
            for i in 0..n {
                ps.push(Port::new(40, y0 + 10 * i, PortKind::Output, o.width));
            }
            sx = 20;
            sy = mult * (y0 + 10 * n);
        }
        ps.push(Port::new(sx, sy, PortKind::Input, BitWidth::new(o.select_bits)));
        if o.enable {
            ps.push(Port::new(sx - 10, sy, PortKind::Input, BitWidth::ONE));
        }
        ps.push(Port::new(0, 0, PortKind::Input, o.width));
        ps
    }

    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let o = &self.opts;
        let n = self.outputs();
        let en = if o.enable { ctx.port_value(n + 1) } else { Value::TRUE };
        let connected = o.enable && ctx.is_port_connected(n + 1);
        let mut others = if o.tristate { Value::unknown(o.width) } else { Value::known(o.width, 0) };
        let mut selected: Option<(usize, Value)> = None;
        match classify_enable(en, connected) {
            Enabled::Off => others = Value::repeat(o.disabled.bit(), o.width),
            Enabled::Error => others = Value::error(o.width),
            Enabled::On => {
                let sel = ctx.port_value(n);
                if sel.is_fully_defined() {
                    let input = n + if o.enable { 2 } else { 1 };
                    selected = Some((sel.to_u64().unwrap() as usize, ctx.port_value(input)));
                } else if sel.is_error() {
                    others = Value::error(o.width);
                } else {
                    others = Value::unknown(o.width);
                }
            }
        }
        for i in 0..n {
            let v = match selected {
                Some((idx, v)) if idx == i => v,
                _ => others,
            };
            ctx.set_port(i, v, DELAY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plexers::testutil::Rig;

    #[test]
    fn two_output_truth_table() {
        for (d, s) in (0..4).map(|i| (i & 2 != 0, i & 1 != 0)) {
            let dv = if d { Value::TRUE } else { Value::FALSE };
            let sv = if s { Value::TRUE } else { Value::FALSE };
            let mut r = Rig::new(
                Box::new(Demultiplexer::new(DemuxOptions::default())),
                &[(2, sv), (3, dv)],
            );
            assert_eq!(r.get(0), if s { Value::FALSE } else { dv });
            assert_eq!(r.get(1), if s { dv } else { Value::FALSE });
        }
    }

    #[test]
    fn four_output_bus_and_tristate() {
        let w = BitWidth::new(3);
        for tri in [false, true] {
            let opts = DemuxOptions::default().select_bits(2).width(3).tristate(tri);
            let other = if tri { Value::unknown(w) } else { Value::known(w, 0) };
            for sel in 0..4u64 {
                let mut r = Rig::new(
                    Box::new(Demultiplexer::new(opts.clone())),
                    &[(4, Value::known(BitWidth::new(2), sel)), (5, Value::known(w, 6))],
                );
                for i in 0..4 {
                    let want = if i as u64 == sel { Value::known(w, 6) } else { other };
                    assert_eq!(r.get(i), want, "sel={sel} out={i} tri={tri}");
                }
            }
        }
    }

    #[test]
    fn enable_unknown_error_select() {
        let opts = DemuxOptions::default().enable(true).disabled(DisabledOutput::Floating);
        let mut r = Rig::new(
            Box::new(Demultiplexer::new(opts)),
            &[(2, Value::FALSE), (3, Value::TRUE), (4, Value::TRUE)],
        );
        assert_eq!(r.get(0), Value::TRUE);
        r.set(3, Value::FALSE);
        assert_eq!((r.get(0), r.get(1)), (Value::UNKNOWN, Value::UNKNOWN));
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
        let d = Demultiplexer::new(DemuxOptions::default().enable(true));
        let p: Vec<_> = d.ports().iter().map(|p| (p.offset.x, p.offset.y)).collect();
        assert_eq!(p, [(30, -10), (30, 10), (20, 20), (10, 20), (0, 0)]);
    }
}
