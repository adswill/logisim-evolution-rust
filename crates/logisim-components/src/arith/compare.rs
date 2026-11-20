//! Comparator and Min/Max.

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

use super::{delay, raw, sext, Sign, SignOptions};

/// Magnitude comparator with `>`, `=`, `<` outputs. Default: 8 bits, signed (as in Logisim).
///
/// Ports: 0 `=` (0,0); 1 `>` (0,-10); 2 `<` (0,10); 3 A (-40,-10); 4 B (-40,10).
#[derive(Debug, Clone)]
pub struct Comparator {
    opts: SignOptions,
}

impl Default for Comparator {
    fn default() -> Self {
        Comparator { opts: SignOptions::default().signed() }
    }
}

impl Comparator {
    pub const EQ: usize = 0;
    pub const GT: usize = 1;
    pub const LT: usize = 2;
    pub const A: usize = 3;
    pub const B: usize = 4;
    pub fn new(opts: SignOptions) -> Self {
        Comparator { opts }
    }
}

impl Component for Comparator {
    fn name(&self) -> &str {
        "Comparator"
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![super::width_spec(8), super::sign_spec("signed")]
    }
    fn options(&self) -> OptionMap {
        self.opts.options()
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        self.opts.apply(m)
    }
    fn ports(&self) -> Vec<Port> {
        let w = self.opts.width.width;
        let one = BitWidth::ONE;
        vec![
            Port::new(0, 0, PortKind::Output, one),
            Port::new(0, -10, PortKind::Output, one),
            Port::new(0, 10, PortKind::Output, one),
            Port::new(-40, -10, PortKind::Input, w),
            Port::new(-40, 10, PortKind::Input, w),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let (mut gt, mut eq, mut lt) = (Value::FALSE, Value::TRUE, Value::FALSE);
        let ax = ctx.port_value(Self::A).bits();
        let bx = ctx.port_value(Self::B).bits();
        let maxlen = ax.len().max(bx.len());
        for pos in (0..maxlen).rev() {
            let mut ab = ax.get(pos).copied().unwrap_or(Value::ERROR);
            let mut bb = bx.get(pos).copied().unwrap_or(Value::ERROR);
            // sign bit: for signed compares a set sign bit means "smaller"
            if pos + 1 == ax.len() && ab != bb && self.opts.sign == Sign::Signed {
                std::mem::swap(&mut ab, &mut bb);
            }
            if ab == Value::ERROR || bb == Value::ERROR {
                (gt, eq, lt) = (Value::ERROR, Value::ERROR, Value::ERROR);
                break;
            } else if ab == Value::UNKNOWN || bb == Value::UNKNOWN {
                (gt, eq, lt) = (Value::UNKNOWN, Value::UNKNOWN, Value::UNKNOWN);
                break;
            } else if ab != bb {
                eq = Value::FALSE;
                if ab == Value::TRUE {
                    gt = Value::TRUE;
                } else {
                    lt = Value::TRUE;
                }
                break;
            }
        }
        let d = delay(self.opts.width.width.get() as usize + 2);
        ctx.set_port(Self::GT, gt, d);
        ctx.set_port(Self::EQ, eq, d);
        ctx.set_port(Self::LT, lt, d);
    }
}

/// Outputs the smaller and larger of two inputs. Default: 8 bits, unsigned.
///
/// Ports: 0 min (0,-10); 1 max (0,10); 2 A (-40,-10); 3 B (-40,10). Undefined inputs give errors.
#[derive(Debug, Clone, Default)]
pub struct MinMax {
    opts: SignOptions,
}

impl MinMax {
    pub const MIN: usize = 0;
    pub const MAX: usize = 1;
    pub const A: usize = 2;
    pub const B: usize = 3;
    pub fn new(opts: SignOptions) -> Self {
        MinMax { opts }
    }
}

impl Component for MinMax {
    fn name(&self) -> &str {
        "Min/Max"
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![super::width_spec(8), super::sign_spec("unsigned")]
    }
    fn options(&self) -> OptionMap {
        self.opts.options()
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        self.opts.apply(m)
    }
    fn ports(&self) -> Vec<Port> {
        let w = self.opts.width.width;
        vec![
            Port::new(0, -10, PortKind::Output, w),
            Port::new(0, 10, PortKind::Output, w),
            Port::new(-40, -10, PortKind::Input, w),
            Port::new(-40, 10, PortKind::Input, w),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let w = self.opts.width.width;
        let (a, b) = (ctx.port_value(Self::A), ctx.port_value(Self::B));
        let (min, max) = if a.is_fully_defined() && b.is_fully_defined() {
            let (ra, rb) = (raw(&a), raw(&b));
            let (lo, hi) = match self.opts.sign {
                Sign::Unsigned => (ra.min(rb), ra.max(rb)),
                Sign::Signed => {
                    let (sa, sb) = (sext(ra, w.get()), sext(rb, w.get()));
                    (sa.min(sb) as u64, sa.max(sb) as u64)
                }
            };
            (Value::known(w, lo), Value::known(w, hi))
        } else {
            (Value::error(w), Value::error(w))
        };
        let d = delay(w.get() as usize + 2);
        ctx.set_port(Self::MIN, min, d);
        ctx.set_port(Self::MAX, max, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arith::testutil::{eval, k};

    fn cmp(sign: Sign, a: u64, b: u64) -> [Value; 3] {
        let c = Comparator::new(SignOptions::default().width(4).sign(sign));
        let o = eval(c, &[(3, k(4, a)), (4, k(4, b))], &[0, 1, 2]);
        [o[0], o[1], o[2]] // eq, gt, lt
    }

    #[test]
    fn comparator_unsigned() {
        let (t, f) = (Value::TRUE, Value::FALSE);
        assert_eq!(cmp(Sign::Unsigned, 5, 5), [t, f, f]);
        assert_eq!(cmp(Sign::Unsigned, 9, 5), [f, t, f]);
        assert_eq!(cmp(Sign::Unsigned, 5, 9), [f, f, t]);
    }

    #[test]
    fn comparator_signed() {
        let (t, f) = (Value::TRUE, Value::FALSE);
        // 9 = -7 in 4-bit two's complement
        assert_eq!(cmp(Sign::Signed, 9, 5), [f, f, t]);
        assert_eq!(cmp(Sign::Signed, 5, 9), [f, t, f]);
        assert_eq!(cmp(Sign::Signed, 15, 14), [f, t, f]); // -1 > -2
        assert_eq!(cmp(Sign::Signed, 3, 3), [t, f, f]);
    }

    #[test]
    fn comparator_default_is_signed_and_undefined_inputs() {
        let o = eval(Comparator::default(), &[(3, k(8, 0xFF)), (4, k(8, 1))], &[1, 2]);
        assert_eq!(o, vec![Value::FALSE, Value::TRUE]);
        // an unknown bit that matters makes everything unknown; a higher differing bit decides first
        let a = Value::create(4, 0, 0b0001, 0b0100);
        let c = Comparator::new(SignOptions::default().width(4).unsigned());
        assert_eq!(eval(c.clone(), &[(3, a), (4, k(4, 0b0100))], &[0, 1, 2]), vec![Value::UNKNOWN; 3]);
        assert_eq!(eval(c, &[(3, a), (4, k(4, 0b1000))], &[0, 1, 2]), vec![Value::FALSE, Value::FALSE, Value::TRUE]);
    }

    #[test]
    fn minmax() {
        let u = MinMax::new(SignOptions::default().width(4));
        assert_eq!(eval(u, &[(2, k(4, 9)), (3, k(4, 5))], &[0, 1]), vec![k(4, 5), k(4, 9)]);
        let s = MinMax::new(SignOptions::default().width(4).signed());
        assert_eq!(eval(s, &[(2, k(4, 9)), (3, k(4, 5))], &[0, 1]), vec![k(4, 9), k(4, 5)]);
        let u = MinMax::new(SignOptions::default().width(4));
        let o = eval(u, &[(2, Value::create(4, 0, 1, 0)), (3, k(4, 5))], &[0, 1]);
        assert!(o[0].is_error() && o[1].is_error());
    }
}
