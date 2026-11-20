//! Bit-level arithmetic parts: Shifter, Bit Adder and Bit Finder.

use logisim_core::{BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{delay, raw, sext, WidthOptions};

/// Which way and how the [`Shifter`] moves bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShiftKind {
    #[default]
    LogicalLeft,
    LogicalRight,
    ArithmeticRight,
    RollLeft,
    RollRight,
}

#[derive(Debug, Clone, Default)]
pub struct ShifterOptions {
    pub width: WidthOptions,
    pub kind: ShiftKind,
}

impl ShifterOptions {
    pub fn width(mut self, w: u8) -> Self {
        self.width = self.width.width(w);
        self
    }
    pub fn kind(mut self, k: ShiftKind) -> Self {
        self.kind = k;
        self
    }
}

/// Number of distance bits for a shifter of `data` bits (smallest `s >= 1` with `2^s >= data`).
pub fn shift_bits(data: u8) -> u8 {
    let mut s = 1;
    while (1u32 << s) < u32::from(data) {
        s += 1;
    }
    s
}

/// Barrel shifter.
///
/// Ports: 0 out (0,0); 1 data in (-40,-10); 2 shift distance (-40,10), `ceil(log2(width))` bits.
/// Defaults: 8 bits, logical left.
#[derive(Debug, Clone, Default)]
pub struct Shifter {
    opts: ShifterOptions,
}

impl Shifter {
    pub const OUT: usize = 0;
    pub const DATA: usize = 1;
    pub const DIST: usize = 2;
    pub fn new(opts: ShifterOptions) -> Self {
        Shifter { opts }
    }

    fn shift_defined(kind: ShiftKind, bits: u8, x: u64, d: u32) -> u64 {
        let b = u32::from(bits);
        let shl = |x: u64, n: u32| if n >= 64 { 0 } else { x << n };
        let shr = |x: u64, n: u32| if n >= 64 { 0 } else { x >> n };
        let y = match kind {
            ShiftKind::LogicalRight => shr(x, d),
            ShiftKind::ArithmeticRight => (sext(x, bits) >> d.min(b - 1).min(63)) as u64,
            ShiftKind::RollRight => {
                let d = if d >= b { d - b } else { d };
                if d == 0 {
                    x
                } else {
                    shr(x, d) | shl(x, b - d)
                }
            }
            ShiftKind::RollLeft => {
                let d = if d >= b { d - b } else { d };
                if d == 0 {
                    x
                } else {
                    shl(x, d) | shr(x, b - d)
                }
            }
            ShiftKind::LogicalLeft => shl(x, d),
        };
        Value::known(BitWidth::new(bits), y).to_u64().unwrap_or(0)
    }

    fn shift_bitwise(kind: ShiftKind, x: &[Value], d: usize) -> Vec<Value> {
        let n = x.len();
        let mut y = vec![Value::FALSE; n];
        match kind {
            ShiftKind::LogicalRight => {
                let d = d.min(n);
                y[..n - d].copy_from_slice(&x[d..]);
            }
            ShiftKind::ArithmeticRight => {
                let d = d.min(n);
                y[..n - d].copy_from_slice(&x[d..]);
                y[n - d..].fill(x[n - 1]);
            }
            ShiftKind::RollRight => {
                let d = if d >= n { d - n } else { d };
                y[..n - d].copy_from_slice(&x[d..]);
                y[n - d..].copy_from_slice(&x[..d]);
            }
            ShiftKind::RollLeft => {
                let d = if d >= n { d - n } else { d };
                y[..d].copy_from_slice(&x[n - d..]);
                y[d..].copy_from_slice(&x[..n - d]);
            }
            ShiftKind::LogicalLeft => {
                let d = d.min(n);
                y[d..].copy_from_slice(&x[..n - d]);
            }
        }
        y
    }
}

impl Component for Shifter {
    fn name(&self) -> &str {
        "Shifter"
    }
    fn ports(&self) -> Vec<Port> {
        let w = self.opts.width.width;
        vec![
            Port::new(0, 0, PortKind::Output, w),
            Port::new(-40, -10, PortKind::Input, w),
            Port::new(-40, 10, PortKind::Input, BitWidth::new(shift_bits(w.get()))),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let width = self.opts.width.width;
        let bits = width.get();
        let (vx, vd) = (ctx.port_value(Self::DATA), ctx.port_value(Self::DIST));
        let kind = self.opts.kind;
        let vy = if vd.is_fully_defined() && vx.width() == width {
            let d = raw(&vd) as u32;
            if d == 0 {
                vx
            } else if vx.is_fully_defined() {
                Value::known(width, Self::shift_defined(kind, bits, raw(&vx), d))
            } else {
                Value::from_bits(&Self::shift_bitwise(kind, &vx.bits(), d as usize))
            }
        } else {
            Value::error(width)
        };
        ctx.set_port(Self::OUT, vy, delay(bits as usize * 3));
    }
}

#[derive(Debug, Clone)]
pub struct BitAdderOptions {
    pub width: WidthOptions,
    /// Number of input ports, 1..=64.
    pub inputs: usize,
}

impl Default for BitAdderOptions {
    fn default() -> Self {
        BitAdderOptions { width: WidthOptions::default(), inputs: 1 }
    }
}

impl BitAdderOptions {
    pub fn width(mut self, w: u8) -> Self {
        self.width = self.width.width(w);
        self
    }
    pub fn inputs(mut self, n: usize) -> Self {
        assert!((1..=64).contains(&n), "bit adder takes 1..=64 inputs");
        self.inputs = n;
        self
    }
}

/// Counts the 1 bits across all inputs.
///
/// Ports: 0 count out (0,0); 1..=n inputs on the left, spaced 10 apart (20 apart for two inputs).
/// Defaults: 8-bit inputs, one input.
#[derive(Debug, Clone, Default)]
pub struct BitAdder {
    opts: BitAdderOptions,
}

impl BitAdder {
    pub fn new(opts: BitAdderOptions) -> Self {
        BitAdder { opts }
    }
    fn output_bits(&self) -> u8 {
        let max = self.opts.width.width.get() as usize * self.opts.inputs;
        let mut out = 1u8;
        while (1usize << out) <= max {
            out += 1;
        }
        out
    }
}

impl Component for BitAdder {
    fn name(&self) -> &str {
        "Bit Adder"
    }
    fn ports(&self) -> Vec<Port> {
        let n = self.opts.inputs as i32;
        let (y, dy) = match n {
            1 => (0, 10),
            2 => (-10, 20),
            3 => (-10, 10),
            _ => (((n - 1) / 2) * -10, 10),
        };
        let mut ps = vec![Port::new(0, 0, PortKind::Output, BitWidth::new(self.output_bits()))];
        ps.extend((0..n).map(|i| Port::new(-40, y + i * dy, PortKind::Input, self.opts.width.width)));
        ps
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let (mut min_count, mut max_count) = (0u64, 0u64);
        for i in 1..=self.opts.inputs {
            for b in ctx.port_value(i).bits() {
                min_count += u64::from(b == Value::TRUE);
                max_count += u64::from(b != Value::FALSE);
            }
        }
        let unknown_mask = (min_count + 1..=max_count).fold(0, |m, i| m | (min_count ^ i));
        let n = self.output_bits();
        let out: Vec<Value> = (0..n)
            .map(|i| {
                if (unknown_mask >> i) & 1 == 1 {
                    Value::ERROR
                } else if (min_count >> i) & 1 == 1 {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            })
            .collect();
        ctx.set_port(0, Value::from_bits(&out), delay(n as usize));
    }
}

/// What the [`BitFinder`] looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BitFinderType {
    /// Lowest-order 1 (default).
    #[default]
    LowOne,
    HighOne,
    LowZero,
    HighZero,
}

#[derive(Debug, Clone, Default)]
pub struct BitFinderOptions {
    pub width: WidthOptions,
    pub find: BitFinderType,
}

impl BitFinderOptions {
    pub fn width(mut self, w: u8) -> Self {
        self.width = self.width.width(w);
        self
    }
    pub fn find(mut self, t: BitFinderType) -> Self {
        self.find = t;
        self
    }
}

/// Finds the index of the lowest/highest 1 (or 0) bit.
///
/// Ports: 0 index (0,0), `ceil(log2(width))` bits; 1 "present" (-20,20); 2 input (-40,0).
/// Defaults: 8 bits, lowest-order 1.
#[derive(Debug, Clone, Default)]
pub struct BitFinder {
    opts: BitFinderOptions,
}

impl BitFinder {
    pub const INDEX: usize = 0;
    pub const PRESENT: usize = 1;
    pub const INPUT: usize = 2;
    pub fn new(opts: BitFinderOptions) -> Self {
        BitFinder { opts }
    }
    fn output_bits(&self) -> BitWidth {
        let mut out = 1u8;
        while (1u64 << out) < u64::from(self.opts.width.width.get()) {
            out += 1;
        }
        BitWidth::new(out)
    }
}

impl Component for BitFinder {
    fn name(&self) -> &str {
        "Bit Finder"
    }
    fn ports(&self) -> Vec<Port> {
        vec![
            Port::new(0, 0, PortKind::Output, self.output_bits()),
            Port::new(-20, 20, PortKind::Output, BitWidth::ONE),
            Port::new(-40, 0, PortKind::Input, self.opts.width.width),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let bits = ctx.port_value(Self::INPUT).bits();
        let ow = self.output_bits();
        let (want, skip, from_high) = match self.opts.find {
            BitFinderType::HighZero => (Value::FALSE, Value::TRUE, true),
            BitFinderType::LowZero => (Value::FALSE, Value::TRUE, false),
            BitFinderType::HighOne => (Value::TRUE, Value::FALSE, true),
            BitFinderType::LowOne => (Value::TRUE, Value::FALSE, false),
        };
        let mut order: Box<dyn Iterator<Item = usize>> =
            if from_high { Box::new((0..bits.len()).rev()) } else { Box::new(0..bits.len()) };
        // first bit that is not `skip` (the opposite of what we want)
        let found = order.find(|&i| bits[i] != skip);
        let (present, index) = match found {
            None => (Value::FALSE, Value::known(ow, 0)),
            Some(i) if bits[i] == want => (Value::TRUE, Value::known(ow, i as u64)),
            Some(_) => (Value::ERROR, Value::error(ow)),
        };
        let d = delay(ow.get() as usize);
        ctx.set_port(Self::PRESENT, present, d);
        ctx.set_port(Self::INDEX, index, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arith::testutil::{eval, k};

    fn shift(kind: ShiftKind, w: u8, x: Value, d: u64) -> Value {
        let c = Shifter::new(ShifterOptions::default().width(w).kind(kind));
        let dw = shift_bits(w);
        eval(c, &[(1, x), (2, k(dw, d))], &[0])[0]
    }

    #[test]
    fn shifter_ports_and_kinds() {
        assert_eq!(shift_bits(8), 3);
        assert_eq!(shift_bits(5), 3);
        assert_eq!(shift_bits(32), 5);
        assert_eq!(shift_bits(1), 1);
        let x = k(8, 0b1001_0110);
        assert_eq!(shift(ShiftKind::LogicalLeft, 8, x, 3), k(8, 0b1011_0000));
        assert_eq!(shift(ShiftKind::LogicalRight, 8, x, 3), k(8, 0b0001_0010));
        assert_eq!(shift(ShiftKind::ArithmeticRight, 8, x, 3), k(8, 0b1111_0010));
        assert_eq!(shift(ShiftKind::RollLeft, 8, x, 3), k(8, 0b1011_0100));
        assert_eq!(shift(ShiftKind::RollRight, 8, x, 3), k(8, 0b1101_0010));
        assert_eq!(shift(ShiftKind::LogicalLeft, 8, x, 0), x);
        assert_eq!(shift(ShiftKind::ArithmeticRight, 8, x, 7), k(8, 0xFF));
        assert_eq!(shift(ShiftKind::LogicalLeft, 64, k(64, 1), 63), k(64, 1 << 63));
        assert_eq!(shift(ShiftKind::ArithmeticRight, 64, k(64, 1 << 63), 4), k(64, 0xF8 << 56));
    }

    #[test]
    fn shifter_unknown_bits_and_bad_distance() {
        let x = Value::create(8, 0, 0b0000_0001, 0b1000_0000);
        assert_eq!(shift(ShiftKind::LogicalLeft, 8, x, 1).to_binary_string(), "0000001x".replace("0000001x", "000000x0"));
        assert_eq!(shift(ShiftKind::LogicalRight, 8, x, 7).to_binary_string(), "0000001x".replace("0000001x", "00000001"));
        let c = Shifter::new(ShifterOptions::default());
        let bad = eval(c, &[(1, k(8, 1)), (2, Value::create(3, 0, 1, 0))], &[0]);
        assert_eq!(bad[0], Value::error(BitWidth::new(8)));
    }

    fn count(width: u8, ins: &[Value]) -> Value {
        let c = BitAdder::new(BitAdderOptions::default().width(width).inputs(ins.len()));
        let ports: Vec<(usize, Value)> = ins.iter().enumerate().map(|(i, v)| (i + 1, *v)).collect();
        eval(c, &ports, &[0])[0]
    }

    #[test]
    fn bit_adder_counts_ones() {
        assert_eq!(count(8, &[k(8, 0b1011_0101)]), k(4, 5));
        assert_eq!(count(4, &[k(4, 0xF), k(4, 0xF), k(4, 0x7)]), k(4, 11));
        assert_eq!(BitAdder::default().ports()[0].width, BitWidth::new(4));
        assert_eq!(BitAdder::new(BitAdderOptions::default().inputs(2)).ports()[1].offset.y, -10);
        // one unknown bit: the count is 2 or 3, so only bit 0 is uncertain
        let v = Value::create(4, 0, 0b0001, 0b0110);
        let out = count(4, &[v]);
        assert_eq!(out.bit(0), Value::ERROR);
        assert_eq!(out.bit(1), Value::TRUE); // two definite ones
        assert_eq!(out.bit(2), Value::FALSE);
    }

    fn find(t: BitFinderType, x: Value) -> Vec<Value> {
        let c = BitFinder::new(BitFinderOptions::default().find(t));
        eval(c, &[(2, x)], &[1, 0])
    }

    #[test]
    fn bit_finder() {
        let x = k(8, 0b0010_1000);
        assert_eq!(find(BitFinderType::LowOne, x), vec![Value::TRUE, k(3, 3)]);
        assert_eq!(find(BitFinderType::HighOne, x), vec![Value::TRUE, k(3, 5)]);
        assert_eq!(find(BitFinderType::LowZero, x), vec![Value::TRUE, k(3, 0)]);
        assert_eq!(find(BitFinderType::HighZero, x), vec![Value::TRUE, k(3, 7)]);
        assert_eq!(find(BitFinderType::LowOne, k(8, 0)), vec![Value::FALSE, k(3, 0)]);
        assert_eq!(find(BitFinderType::HighZero, k(8, 0xFF)), vec![Value::FALSE, k(3, 0)]);
        // an unknown bit encountered before a 1 poisons the answer
        let y = Value::create(8, 0, 0b0000_0100, 0b0100_0000);
        let o = find(BitFinderType::LowOne, y);
        assert_eq!(o[0], Value::ERROR);
        assert!(o[1].is_error());
    }
}
