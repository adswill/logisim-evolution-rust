//! Multiplier, Divider, Exponentiator and Square Root.

use logisim_core::{BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{delay, is_absent, raw, sext, undefined_like, Sign, SignOptions, WidthOptions};

fn has_error(v: &Value) -> bool {
    v.bits().contains(&Value::ERROR)
}

/// Index of the first bit that is not 0/1 (`len` if all bits are defined).
fn first_undefined(v: &Value) -> usize {
    v.bits().iter().position(|b| *b != Value::TRUE && *b != Value::FALSE).unwrap_or(v.width().get() as usize)
}

fn first_error(v: &Value) -> usize {
    v.bits().iter().position(|b| *b == Value::ERROR).unwrap_or(v.width().get() as usize)
}

/// Operand as a signed 128-bit integer (X/E bits read as 0).
fn big(v: &Value, unsigned: bool) -> i128 {
    let r = raw(v);
    if unsigned {
        i128::from(r)
    } else {
        i128::from(sext(r, v.width().get()))
    }
}

/// How the multiplier interprets its operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MulMode {
    /// Both operands two's complement.
    Signed,
    /// A signed, B unsigned.
    SignedUnsigned,
    /// Both operands unsigned (default).
    #[default]
    Unsigned,
}

#[derive(Debug, Clone, Default)]
pub struct MultiplierOptions {
    pub width: WidthOptions,
    pub mode: MulMode,
}

impl MultiplierOptions {
    pub fn width(mut self, w: u8) -> Self {
        self.width = self.width.width(w);
        self
    }
    pub fn mode(mut self, m: MulMode) -> Self {
        self.mode = m;
        self
    }
}

/// `a * b + c_in` over `width` bits; returns `(low, high)`. Port of `Multiplier.computeProduct`.
pub(crate) fn compute_product(
    width: BitWidth,
    a: Value,
    b: Value,
    mut c_in: Value,
    unsigned_a: bool,
    unsigned_b: bool,
) -> (Value, Value) {
    let w = width.get();
    if is_absent(c_in) {
        c_in = Value::known(width, 0);
    }
    let unsigned_c = unsigned_a && unsigned_b;
    let product = |a: &Value, b: &Value, c: &Value| -> (u64, u64) {
        if unsigned_c {
            let rr = (raw(a) as u128).wrapping_mul(raw(b) as u128).wrapping_add(raw(c) as u128);
            (rr as u64, (rr >> w) as u64)
        } else {
            let rr = big(a, unsigned_a).wrapping_mul(big(b, unsigned_b)).wrapping_add(big(c, false));
            (rr as u64, (rr >> w) as u64)
        }
    };
    if a.is_fully_defined() && b.is_fully_defined() && c_in.is_fully_defined() {
        let (lo, hi) = product(&a, &b, &c_in);
        return (Value::known(width, lo), Value::known(width, hi));
    }
    let known = first_undefined(&a).min(first_undefined(&b)).min(first_undefined(&c_in));
    let error = first_error(&a).min(first_error(&b)).min(first_error(&c_in));
    let (lo, _) = product(&a, &b, &c_in);
    let bits: Vec<Value> = (0..w as usize)
        .map(|i| {
            if i < known {
                if (lo >> i) & 1 == 1 {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            } else if i < error {
                Value::UNKNOWN
            } else {
                Value::ERROR
            }
        })
        .collect();
    let carry = if error < w as usize { Value::error(width) } else { Value::unknown(width) };
    (Value::from_bits(&bits), carry)
}

/// Multiplier with carry in/out (the carry in is added; the carry out is the high half).
///
/// Ports: 0 product low (0,0); 1 product high / carry out (-20,20); 2 A (-40,-10); 3 B (-40,10);
/// 4 carry in (-20,-20). Defaults: 8 bits, unsigned.
#[derive(Debug, Clone, Default)]
pub struct Multiplier {
    opts: MultiplierOptions,
}

impl Multiplier {
    pub const OUT: usize = 0;
    pub const C_OUT: usize = 1;
    pub const A: usize = 2;
    pub const B: usize = 3;
    pub const C_IN: usize = 4;
    pub fn new(opts: MultiplierOptions) -> Self {
        Multiplier { opts }
    }
}

impl Component for Multiplier {
    fn name(&self) -> &str {
        "Multiplier"
    }
    fn ports(&self) -> Vec<Port> {
        let w = self.opts.width.width;
        vec![
            Port::new(0, 0, PortKind::Output, w),
            Port::new(-20, 20, PortKind::Output, w),
            Port::new(-40, -10, PortKind::Input, w),
            Port::new(-40, 10, PortKind::Input, w),
            Port::new(-20, -20, PortKind::Input, w),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let w = self.opts.width.width;
        let unsigned_a = self.opts.mode == MulMode::Unsigned;
        let unsigned_b = self.opts.mode != MulMode::Signed;
        let (lo, hi) = compute_product(
            w,
            ctx.port_value(Self::A),
            ctx.port_value(Self::B),
            ctx.port_value(Self::C_IN),
            unsigned_a,
            unsigned_b,
        );
        let d = delay(w.get() as usize * (w.get() as usize + 2));
        ctx.set_port(Self::OUT, lo, d);
        ctx.set_port(Self::C_OUT, hi, d);
    }
}

/// Divider: `(upper:A) / B` giving quotient and remainder. Division by zero divides by one.
///
/// Ports: 0 quotient (0,0); 1 remainder (-20,20); 2 A / dividend low (-40,-10); 3 B / divisor
/// (-40,10); 4 dividend upper half (-20,-20). Defaults: 8 bits, unsigned.
#[derive(Debug, Clone, Default)]
pub struct Divider {
    opts: SignOptions,
}

impl Divider {
    pub const OUT: usize = 0;
    pub const REM: usize = 1;
    pub const A: usize = 2;
    pub const B: usize = 3;
    pub const UPPER: usize = 4;
    pub fn new(opts: SignOptions) -> Self {
        Divider { opts }
    }

    /// Port of `Divider.computeResult`; returns `(quotient, remainder)`.
    pub(crate) fn compute(width: BitWidth, a: Value, b: Value, mut upper: Value, unsigned: bool) -> (Value, Value) {
        let w = width.get();
        let has_upper = !is_absent(upper);
        if !has_upper {
            upper = Value::known(width, 0);
        }
        if a.is_fully_defined() && b.is_fully_defined() && upper.is_fully_defined() {
            let (res, rem) = if unsigned {
                let den = if raw(&b) == 0 { 1 } else { u128::from(raw(&b)) };
                let num = if has_upper { (u128::from(raw(&upper)) << w) | u128::from(raw(&a)) } else { u128::from(raw(&a)) };
                ((num / den) as u64, (num % den) as u64)
            } else {
                let sb = big(&b, false);
                let den = if sb == 0 { 1 } else { sb };
                let num = if has_upper { (big(&upper, false) << w) | i128::from(raw(&a)) } else { big(&a, false) };
                ((num / den) as u64, (num % den) as u64)
            };
            (Value::known(width, res), Value::known(width, rem))
        } else {
            let v = undefined_like(width, has_error(&a) || has_error(&b) || has_error(&upper));
            (v, v)
        }
    }
}

impl Component for Divider {
    fn name(&self) -> &str {
        "Divider"
    }
    fn ports(&self) -> Vec<Port> {
        let w = self.opts.width.width;
        vec![
            Port::new(0, 0, PortKind::Output, w),
            Port::new(-20, 20, PortKind::Output, w),
            Port::new(-40, -10, PortKind::Input, w),
            Port::new(-40, 10, PortKind::Input, w),
            Port::new(-20, -20, PortKind::Input, w),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let w = self.opts.width.width;
        let (q, r) = Self::compute(
            w,
            ctx.port_value(Self::A),
            ctx.port_value(Self::B),
            ctx.port_value(Self::UPPER),
            self.opts.sign == Sign::Unsigned,
        );
        let d = delay(w.get() as usize * (w.get() as usize + 2));
        ctx.set_port(Self::OUT, q, d);
        ctx.set_port(Self::REM, r, d);
    }
}

/// Integer power `a ^ b` (wrapping), `0` for negative exponents.
fn pow_i64(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0;
    }
    let (mut result, mut b, mut e) = (1i64, base, exp as u64);
    while e != 0 {
        if e & 1 != 0 {
            result = result.wrapping_mul(b);
        }
        e >>= 1;
        if e != 0 {
            b = b.wrapping_mul(b);
        }
    }
    result
}

/// `base ^ exp mod 2^bits` for `bits <= 128`.
fn modpow_pow2(base: u128, mut exp: u64, bits: u32) -> u128 {
    let mask = if bits >= 128 { u128::MAX } else { (1u128 << bits) - 1 };
    let (mut result, mut b) = (1u128 & mask, base & mask);
    while exp != 0 {
        if exp & 1 != 0 {
            result = result.wrapping_mul(b) & mask;
        }
        exp >>= 1;
        b = b.wrapping_mul(b) & mask;
    }
    result
}

/// Exponentiator: `base ^ exponent`, low and high half of the result.
///
/// Ports: 0 low half (0,0); 1 high half (-20,20); 2 base (-40,-10); 3 exponent (-40,10).
/// Defaults: **32 bits**, unsigned (as in Logisim).
#[derive(Debug, Clone)]
pub struct Exponentiator {
    opts: SignOptions,
}

impl Default for Exponentiator {
    fn default() -> Self {
        Exponentiator { opts: SignOptions::default().width(32) }
    }
}

impl Exponentiator {
    pub const LOW: usize = 0;
    pub const HIGH: usize = 1;
    pub const BASE: usize = 2;
    pub const EXP: usize = 3;
    pub fn new(opts: SignOptions) -> Self {
        Exponentiator { opts }
    }

    /// Port of `Exponentiator.computePower`; returns `(low, high)`.
    pub(crate) fn compute(width: BitWidth, a: Value, b: Value, unsigned: bool) -> (Value, Value) {
        let w = width.get();
        if a.is_fully_defined() && b.is_fully_defined() {
            if raw(&a) == 1 {
                return (Value::known(width, 1), Value::known(width, 0));
            }
            if w <= 32 && !unsigned {
                let rr = pow_i64(sext(raw(&a), w), sext(raw(&b), w));
                return (Value::known(width, rr as u64), Value::known(width, (rr >> w) as u64));
            }
            let exp = if unsigned { Some(raw(&b)) } else { u64::try_from(sext(raw(&b), w)).ok() };
            let Some(exp) = exp else {
                return (Value::known(width, 0), Value::known(width, 0));
            };
            let rr = modpow_pow2(big(&a, unsigned) as u128, exp, 2 * u32::from(w));
            (Value::known(width, rr as u64), Value::known(width, (rr >> w) as u64))
        } else {
            let v = undefined_like(width, has_error(&a) || has_error(&b));
            (v, v)
        }
    }
}

impl Component for Exponentiator {
    fn name(&self) -> &str {
        "Exponentiator"
    }
    fn ports(&self) -> Vec<Port> {
        let w = self.opts.width.width;
        vec![
            Port::new(0, 0, PortKind::Output, w),
            Port::new(-20, 20, PortKind::Output, w),
            Port::new(-40, -10, PortKind::Input, w),
            Port::new(-40, 10, PortKind::Input, w),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let w = self.opts.width.width;
        let (lo, hi) =
            Self::compute(w, ctx.port_value(Self::BASE), ctx.port_value(Self::EXP), self.opts.sign == Sign::Unsigned);
        let d = delay(w.get() as usize + 2);
        ctx.set_port(Self::LOW, lo, d);
        ctx.set_port(Self::HIGH, hi, d);
    }
}

/// Integer square root of the double-width radicand `(upper:in)`, with remainder.
///
/// Ports: 0 root (0,0); 1 remainder (-20,20); 2 radicand low (-40,0); 3 radicand upper (-20,-20).
#[derive(Debug, Clone, Default)]
pub struct SquareRoot {
    opts: WidthOptions,
}

impl SquareRoot {
    pub const OUT: usize = 0;
    pub const REM: usize = 1;
    pub const IN: usize = 2;
    pub const UPPER: usize = 3;
    pub fn new(opts: WidthOptions) -> Self {
        SquareRoot { opts }
    }
}

impl Component for SquareRoot {
    fn name(&self) -> &str {
        "Square Root"
    }
    fn ports(&self) -> Vec<Port> {
        let w = self.opts.width;
        vec![
            Port::new(0, 0, PortKind::Output, w),
            Port::new(-20, 20, PortKind::Output, w),
            Port::new(-40, 0, PortKind::Input, w),
            Port::new(-20, -20, PortKind::Input, w),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let width = self.opts.width;
        let w = width.get();
        let a = ctx.port_value(Self::IN);
        let mut upper = ctx.port_value(Self::UPPER);
        if is_absent(upper) {
            upper = Value::known(width, 0);
        }
        let (root, rem) = if a.is_fully_defined() && upper.is_fully_defined() {
            let num = (u128::from(raw(&upper)) << w) | u128::from(raw(&a));
            let root = num.isqrt();
            (Value::known(width, root as u64), Value::known(width, (num - root * root) as u64))
        } else {
            let v = undefined_like(width, has_error(&a) || has_error(&upper));
            (v, v)
        };
        let d = delay(w as usize * (w as usize + 2));
        ctx.set_port(Self::OUT, root, d);
        ctx.set_port(Self::REM, rem, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arith::testutil::{eval, k};

    fn mul(mode: MulMode, w: u8, a: u64, b: u64, c: Option<u64>) -> Vec<Value> {
        let mut ins = vec![(2, k(w, a)), (3, k(w, b))];
        if let Some(c) = c {
            ins.push((4, k(w, c)));
        }
        eval(Multiplier::new(MultiplierOptions::default().width(w).mode(mode)), &ins, &[0, 1])
    }

    #[test]
    fn multiplier_modes() {
        assert_eq!(mul(MulMode::Unsigned, 4, 15, 15, None), vec![k(4, 1), k(4, 14)]); // 225 = 0xE1
        assert_eq!(mul(MulMode::Unsigned, 4, 3, 4, Some(5)), vec![k(4, 1), k(4, 1)]); // 17 = 0x11
        assert_eq!(mul(MulMode::Signed, 4, 15, 15, None), vec![k(4, 1), k(4, 0)]); // (-1)*(-1)
        assert_eq!(mul(MulMode::Signed, 4, 15, 2, None), vec![k(4, 14), k(4, 15)]); // -2
        assert_eq!(mul(MulMode::SignedUnsigned, 4, 15, 15, None), vec![k(4, 1), k(4, 15)]); // -1 * 15 = -15
    }

    #[test]
    fn multiplier_64_bit() {
        let o = mul(MulMode::Unsigned, 64, u64::MAX, u64::MAX, None);
        assert_eq!(o, vec![k(64, 1), k(64, u64::MAX - 1)]);
        let o = mul(MulMode::Signed, 64, u64::MAX, u64::MAX, None);
        assert_eq!(o, vec![k(64, 1), k(64, 0)]);
    }

    #[test]
    fn multiplier_partial_unknown() {
        // a = 0b00x1 * b = 3: only the lowest bit is certain
        let a = Value::create(4, 0, 0b0010, 0b0001);
        let o = eval(Multiplier::new(MultiplierOptions::default().width(4)), &[(2, a), (3, k(4, 3))], &[0, 1]);
        assert_eq!(o[0].bit(0), Value::TRUE);
        assert_eq!(o[0].bit(1), Value::UNKNOWN);
        assert_eq!(o[1], Value::unknown(BitWidth::new(4)));
    }

    fn div(sign: Sign, w: u8, a: u64, b: u64, upper: Option<u64>) -> Vec<Value> {
        let mut ins = vec![(2, k(w, a)), (3, k(w, b))];
        if let Some(u) = upper {
            ins.push((4, k(w, u)));
        }
        eval(Divider::new(SignOptions::default().width(w).sign(sign)), &ins, &[0, 1])
    }

    #[test]
    fn divider() {
        assert_eq!(div(Sign::Unsigned, 8, 100, 7, None), vec![k(8, 14), k(8, 2)]);
        assert_eq!(div(Sign::Unsigned, 8, 100, 0, None), vec![k(8, 100), k(8, 0)]); // /0 acts as /1
        // upper half: (1:0) / 2 = 256 / 2 = 128
        assert_eq!(div(Sign::Unsigned, 8, 0, 2, Some(1)), vec![k(8, 128), k(8, 0)]);
        // signed: -7 / 2 = -3 rem -1
        assert_eq!(div(Sign::Signed, 8, 0xF9, 2, None), vec![k(8, 0xFD), k(8, 0xFF)]);
        assert_eq!(div(Sign::Unsigned, 64, u64::MAX, 10, Some(0)), vec![k(64, u64::MAX / 10), k(64, u64::MAX % 10)]);
        let o = eval(Divider::default(), &[(2, Value::create(8, 1, 0, 0)), (3, k(8, 2))], &[0, 1]);
        assert_eq!(o[0], Value::error(BitWidth::new(8)));
        let o = eval(Divider::default(), &[(2, Value::create(8, 0, 1, 0)), (3, k(8, 2))], &[0, 1]);
        assert_eq!(o[1], Value::unknown(BitWidth::new(8)));
    }

    fn pow(sign: Sign, w: u8, a: u64, b: u64) -> Vec<Value> {
        let c = Exponentiator::new(SignOptions::default().width(w).sign(sign));
        eval(c, &[(2, k(w, a)), (3, k(w, b))], &[0, 1])
    }

    #[test]
    fn exponentiator() {
        assert_eq!(pow(Sign::Unsigned, 8, 3, 4), vec![k(8, 81), k(8, 0)]);
        assert_eq!(pow(Sign::Unsigned, 8, 2, 10), vec![k(8, 0), k(8, 4)]); // 1024 = 0x400
        assert_eq!(pow(Sign::Unsigned, 8, 1, 200), vec![k(8, 1), k(8, 0)]);
        assert_eq!(pow(Sign::Signed, 8, 0xFE, 3), vec![k(8, 0xF8), k(8, 0xFF)]); // (-2)^3 = -8
        assert_eq!(pow(Sign::Signed, 8, 2, 0xFF), vec![k(8, 0), k(8, 0)]); // negative exponent
        assert_eq!(Exponentiator::default().ports()[0].width, BitWidth::new(32));
    }

    #[test]
    fn square_root() {
        let sq = |w: u8, a: u64, u: Option<u64>| {
            let mut ins = vec![(2, k(w, a))];
            if let Some(u) = u {
                ins.push((3, k(w, u)));
            }
            eval(SquareRoot::new(WidthOptions::default().width(w)), &ins, &[0, 1])
        };
        assert_eq!(sq(8, 100, None), vec![k(8, 10), k(8, 0)]);
        assert_eq!(sq(8, 101, None), vec![k(8, 10), k(8, 1)]);
        assert_eq!(sq(8, 0, Some(1)), vec![k(8, 16), k(8, 0)]); // sqrt(256)
        assert_eq!(sq(64, u64::MAX, Some(u64::MAX)), vec![k(64, u64::MAX), k(64, u64::MAX - 1) /* low 64 bits of 2^65 - 2 */]);
    }
}
