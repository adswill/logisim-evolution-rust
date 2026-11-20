//! Adder, Subtractor, Negator and Absolute value.

use logisim_core::{BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{delay, is_absent, negate_bits, raw, sext, WidthOptions};

/// Sum of `a + b + c_in` over `width` bits; returns `(sum, carry_out)`. Port of `Adder.computeSum`.
pub(crate) fn compute_sum(width: BitWidth, a: Value, b: Value, mut c_in: Value) -> (Value, Value) {
    let w = width.get();
    if is_absent(c_in) && c_in.width().get() <= 1 {
        c_in = Value::FALSE;
    }
    if a.is_fully_defined() && b.is_fully_defined() && c_in.is_fully_defined() {
        let sum = u128::from(raw(&a)) + u128::from(raw(&b)) + u128::from(raw(&c_in));
        let carry = (sum >> w) & 1 == 1;
        return (
            Value::known(width, sum as u64),
            if carry { Value::TRUE } else { Value::FALSE },
        );
    }
    let mut bits = Vec::with_capacity(w as usize);
    let mut carry = c_in;
    for i in 0..w {
        if carry == Value::ERROR {
            bits.push(Value::ERROR);
        } else if carry == Value::UNKNOWN {
            bits.push(Value::UNKNOWN);
        } else {
            let (ab, bb) = (a.bit(i), b.bit(i));
            if ab == Value::ERROR || bb == Value::ERROR {
                bits.push(Value::ERROR);
                carry = Value::ERROR;
            } else if ab == Value::UNKNOWN || bb == Value::UNKNOWN {
                bits.push(Value::UNKNOWN);
                carry = Value::UNKNOWN;
            } else {
                let sum = u8::from(ab == Value::TRUE) + u8::from(bb == Value::TRUE) + u8::from(carry == Value::TRUE);
                bits.push(if sum & 1 == 1 { Value::TRUE } else { Value::FALSE });
                carry = if sum >= 2 { Value::TRUE } else { Value::FALSE };
            }
        }
    }
    (Value::from_bits(&bits), carry)
}

/// 8-bit-by-default adder with carry in/out.
///
/// Ports: 0 sum out (0,0); 1 carry out (-20,20); 2 A (-40,-10); 3 B (-40,10); 4 carry in (-20,-20).
#[derive(Debug, Clone, Default)]
pub struct Adder {
    opts: WidthOptions,
}

impl Adder {
    pub const OUT: usize = 0;
    pub const C_OUT: usize = 1;
    pub const A: usize = 2;
    pub const B: usize = 3;
    pub const C_IN: usize = 4;
    pub fn new(opts: WidthOptions) -> Self {
        Adder { opts }
    }
}

impl Component for Adder {
    fn name(&self) -> &str {
        "Adder"
    }
    fn ports(&self) -> Vec<Port> {
        let w = self.opts.width;
        vec![
            Port::new(0, 0, PortKind::Output, w),
            Port::new(-20, 20, PortKind::Output, BitWidth::ONE),
            Port::new(-40, -10, PortKind::Input, w),
            Port::new(-40, 10, PortKind::Input, w),
            Port::new(-20, -20, PortKind::Input, BitWidth::ONE),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let w = self.opts.width;
        let (sum, cout) = compute_sum(w, ctx.port_value(Self::A), ctx.port_value(Self::B), ctx.port_value(Self::C_IN));
        let d = delay(w.get() as usize + 2);
        ctx.set_port(Self::OUT, sum, d);
        ctx.set_port(Self::C_OUT, cout, d);
    }
}

/// Subtractor `A - B` with borrow in/out.
///
/// Ports: 0 difference out; 1 borrow out (-20,20); 2 A (minuend); 3 B (subtrahend); 4 borrow in (-20,-20).
#[derive(Debug, Clone, Default)]
pub struct Subtractor {
    opts: WidthOptions,
}

impl Subtractor {
    pub const OUT: usize = 0;
    pub const B_OUT: usize = 1;
    pub const A: usize = 2;
    pub const B: usize = 3;
    pub const B_IN: usize = 4;
    pub fn new(opts: WidthOptions) -> Self {
        Subtractor { opts }
    }
}

impl Component for Subtractor {
    fn name(&self) -> &str {
        "Subtractor"
    }
    fn ports(&self) -> Vec<Port> {
        Adder::new(self.opts.clone()).ports()
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let w = self.opts.width;
        let mut b_in = ctx.port_value(Self::B_IN);
        if is_absent(b_in) && b_in.width().get() <= 1 {
            b_in = Value::FALSE;
        }
        let (diff, bout) = compute_sum(w, ctx.port_value(Self::A), !ctx.port_value(Self::B), !b_in);
        let d = delay(w.get() as usize + 4);
        ctx.set_port(Self::OUT, diff, d);
        ctx.set_port(Self::B_OUT, !bout, d);
    }
}

/// Two's-complement negator.
///
/// Ports: 0 out (0,0); 1 in (-40,0).
#[derive(Debug, Clone, Default)]
pub struct Negator {
    opts: WidthOptions,
}

impl Negator {
    pub const OUT: usize = 0;
    pub const IN: usize = 1;
    pub fn new(opts: WidthOptions) -> Self {
        Negator { opts }
    }
}

impl Component for Negator {
    fn name(&self) -> &str {
        "Negator"
    }
    fn ports(&self) -> Vec<Port> {
        let w = self.opts.width;
        vec![Port::new(0, 0, PortKind::Output, w), Port::new(-40, 0, PortKind::Input, w)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let input = ctx.port_value(Self::IN);
        let out = if input.is_fully_defined() {
            Value::known(input.width(), raw(&input).wrapping_neg())
        } else {
            let mut bits = input.bits();
            negate_bits(&mut bits, true);
            Value::from_bits(&bits)
        };
        ctx.set_port(Self::OUT, out, delay(self.opts.width.get() as usize + 2));
    }
}

/// Absolute value with an overflow flag (set for the most negative number).
///
/// Ports: 0 out (0,0); 1 overflow (-20,20); 2 in (-40,0).
///
/// Like the Java original, an input containing X/E bits only has its low bits processed (the
/// tail is not inverted) and overflow is reported as an error.
#[derive(Debug, Clone, Default)]
pub struct Absolute {
    opts: WidthOptions,
}

impl Absolute {
    pub const OUT: usize = 0;
    pub const OVERFLOW: usize = 1;
    pub const IN: usize = 2;
    pub fn new(opts: WidthOptions) -> Self {
        Absolute { opts }
    }
}

impl Component for Absolute {
    fn name(&self) -> &str {
        "Absolute Value"
    }
    fn ports(&self) -> Vec<Port> {
        let w = self.opts.width;
        vec![
            Port::new(0, 0, PortKind::Output, w),
            Port::new(-20, 20, PortKind::Output, BitWidth::ONE),
            Port::new(-40, 0, PortKind::Input, w),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let input = ctx.port_value(Self::IN);
        let (out, overflow) = if input.is_fully_defined() {
            let w = input.width().get();
            let val = sext(raw(&input), w);
            let out = Value::known(input.width(), val.wrapping_abs() as u64);
            let ovf = sext(raw(&out), w) < 0;
            (out, if ovf { Value::TRUE } else { Value::FALSE })
        } else {
            let mut bits = input.bits();
            negate_bits(&mut bits, false);
            (Value::from_bits(&bits), Value::ERROR)
        };
        let d = delay(self.opts.width.get() as usize + 2);
        ctx.set_port(Self::OUT, out, d);
        ctx.set_port(Self::OVERFLOW, overflow, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arith::testutil::{eval, k};

    fn w(n: u8) -> WidthOptions {
        WidthOptions::default().width(n)
    }

    #[test]
    fn adder_sums_and_carries() {
        let o = eval(Adder::new(w(4)), &[(2, k(4, 9)), (3, k(4, 9))], &[0, 1]);
        assert_eq!(o, vec![k(4, 2), Value::TRUE]);
        let o = eval(Adder::new(w(4)), &[(2, k(4, 7)), (3, k(4, 8)), (4, Value::TRUE)], &[0, 1]);
        assert_eq!(o, vec![k(4, 0), Value::TRUE]);
        let o = eval(Adder::new(w(4)), &[(2, k(4, 1)), (3, k(4, 2))], &[0, 1]);
        assert_eq!(o, vec![k(4, 3), Value::FALSE]);
    }

    #[test]
    fn adder_64_bit_carry() {
        let o = eval(Adder::new(w(64)), &[(2, k(64, u64::MAX)), (3, k(64, 1))], &[0, 1]);
        assert_eq!(o, vec![k(64, 0), Value::TRUE]);
    }

    #[test]
    fn adder_unknown_bits_propagate_up_only() {
        // a = 0b01x1 (bit1 unknown): bit 0 is fine, bits 1.. are unknown
        let a = Value::create(4, 0, 0b0010, 0b0101);
        let o = eval(Adder::new(w(4)), &[(2, a), (3, k(4, 0))], &[0, 1]);
        assert_eq!(o[0].bit(0), Value::TRUE);
        assert_eq!(o[0].bit(1), Value::UNKNOWN);
        assert_eq!(o[0].bit(3), Value::UNKNOWN);
        assert_eq!(o[1], Value::UNKNOWN);
    }

    #[test]
    fn subtractor_borrows() {
        let o = eval(Subtractor::new(w(4)), &[(2, k(4, 3)), (3, k(4, 5))], &[0, 1]);
        assert_eq!(o, vec![k(4, 14), Value::TRUE]);
        let o = eval(Subtractor::new(w(4)), &[(2, k(4, 9)), (3, k(4, 5))], &[0, 1]);
        assert_eq!(o, vec![k(4, 4), Value::FALSE]);
        // borrow in subtracts one more
        let o = eval(Subtractor::new(w(4)), &[(2, k(4, 9)), (3, k(4, 5)), (4, Value::TRUE)], &[0, 1]);
        assert_eq!(o, vec![k(4, 3), Value::FALSE]);
    }

    #[test]
    fn negator_defined_and_undefined() {
        assert_eq!(eval(Negator::new(w(8)), &[(1, k(8, 1))], &[0]), vec![k(8, 255)]);
        assert_eq!(eval(Negator::new(w(8)), &[(1, k(8, 0))], &[0]), vec![k(8, 0)]);
        assert_eq!(eval(Negator::new(w(8)), &[(1, k(8, 128))], &[0]), vec![k(8, 128)]);
        // -(0b0x10) : low zero stays, bit1 is the first 1, upper bits inverted (x stays x)
        let v = Value::create(4, 0, 0b0100, 0b0010);
        let o = eval(Negator::new(w(4)), &[(1, v)], &[0]);
        assert_eq!(o[0].to_binary_string(), "1x10");
        // unknown below the first one makes the rest unknown
        let v = Value::create(4, 0, 0b0001, 0b1000);
        let o = eval(Negator::new(w(4)), &[(1, v)], &[0]);
        assert_eq!(o[0].to_binary_string(), "xxxx");
    }

    #[test]
    fn absolute_flags_overflow() {
        assert_eq!(eval(Absolute::new(w(8)), &[(2, k(8, 0xFB))], &[0, 1]), vec![k(8, 5), Value::FALSE]);
        assert_eq!(eval(Absolute::new(w(8)), &[(2, k(8, 5))], &[0, 1]), vec![k(8, 5), Value::FALSE]);
        assert_eq!(eval(Absolute::new(w(8)), &[(2, k(8, 0x80))], &[0, 1]), vec![k(8, 0x80), Value::TRUE]);
        let o = eval(Absolute::new(w(4)), &[(2, Value::create(4, 0, 1, 0))], &[1]);
        assert_eq!(o[0], Value::ERROR);
    }
}
