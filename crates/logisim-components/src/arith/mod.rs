//! The Arithmetic library (port of `com.cburch.logisim.std.arith`).
//!
//! Every part is east-facing with its primary output at (0,0). Port order is documented on each
//! type; **port 0 is always the primary output**, then the remaining outputs, then the inputs
//! (this differs from the Java indices, which listed inputs first).
//!
//! Integer parts: [`Adder`], [`Subtractor`], [`Multiplier`], [`Divider`], [`Negator`],
//! [`Absolute`], [`Comparator`], [`MinMax`], [`Shifter`], [`BitAdder`], [`BitFinder`],
//! [`Exponentiator`], [`SquareRoot`]. Floating-point parts live in [`floating`].

use logisim_core::{BitWidth, Component, OptionMap, Value};

use crate::registry::{Library, PartDesc};

mod basic;
mod bits;
mod compare;
pub mod floating;
mod muldiv;

pub use basic::{Absolute, Adder, Negator, Subtractor};
pub use bits::{BitAdder, BitAdderOptions, BitFinder, BitFinderOptions, BitFinderType, ShiftKind, Shifter, ShifterOptions};
pub use compare::{Comparator, MinMax};
pub use muldiv::{Divider, Exponentiator, MulMode, Multiplier, MultiplierOptions, SquareRoot};

/// Logisim's `PER_DELAY`: ticks per unit of the delay formulas below.
pub const PER_DELAY: u32 = 1;

/// Signed (two's complement) or unsigned interpretation of operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sign {
    Signed,
    #[default]
    Unsigned,
}

/// Options for parts that only have a data width (default 8 bits).
#[derive(Debug, Clone)]
pub struct WidthOptions {
    pub width: BitWidth,
}

impl Default for WidthOptions {
    fn default() -> Self {
        WidthOptions { width: BitWidth::new(8) }
    }
}

impl WidthOptions {
    pub fn width(mut self, w: u8) -> Self {
        assert!((1..=64).contains(&w), "data width must be 1..=64");
        self.width = BitWidth::new(w);
        self
    }
}

pub(crate) const SIGNS: [(&str, Sign); 2] = [("signed", Sign::Signed), ("unsigned", Sign::Unsigned)];

impl WidthOptions {
    /// The saved form: `width`.
    pub(crate) fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width.get())
    }
    /// Reads `width` (rejecting keys other than it and `extra`); absent keys keep the current value.
    pub(crate) fn apply(&mut self, m: &OptionMap, extra: &[&str]) -> Result<(), String> {
        let mut known = vec!["width"];
        known.extend_from_slice(extra);
        m.reject_unknown(&known)?;
        if let Some(w) = m.int("width", 1, 64)? {
            self.width = BitWidth::new(w as u8);
        }
        Ok(())
    }
}

impl SignOptions {
    pub(crate) fn options(&self) -> OptionMap {
        self.width.options().with("sign", OptionMap::name_of(self.sign, &SIGNS))
    }
    pub(crate) fn apply(&mut self, m: &OptionMap) -> Result<(), String> {
        let sign = m.choice("sign", &SIGNS)?.unwrap_or(self.sign);
        self.width.apply(m, &["sign"])?;
        self.sign = sign;
        Ok(())
    }
}

/// Options for parts with a width and a signed/unsigned mode (default 8 bits, unsigned).
#[derive(Debug, Clone, Default)]
pub struct SignOptions {
    pub width: WidthOptions,
    pub sign: Sign,
}

impl SignOptions {
    pub fn width(mut self, w: u8) -> Self {
        self.width = self.width.width(w);
        self
    }
    pub fn sign(mut self, s: Sign) -> Self {
        self.sign = s;
        self
    }
    pub fn signed(self) -> Self {
        self.sign(Sign::Signed)
    }
    pub fn unsigned(self) -> Self {
        self.sign(Sign::Unsigned)
    }
}

/// The Arithmetic library with default options for each part (including floating point).
pub fn library() -> Library {
    let part = |name, make: fn() -> Box<dyn Component>| PartDesc { name, make };
    let mut parts = vec![
        part("Adder", || Box::new(Adder::default())),
        part("Subtractor", || Box::new(Subtractor::default())),
        part("Multiplier", || Box::new(Multiplier::default())),
        part("Divider", || Box::new(Divider::default())),
        part("Negator", || Box::new(Negator::default())),
        part("Exponentiator", || Box::new(Exponentiator::default())),
        part("Square Root", || Box::new(SquareRoot::default())),
        part("Absolute Value", || Box::new(Absolute::default())),
        part("Comparator", || Box::new(Comparator::default())),
        part("Min/Max", || Box::new(MinMax::default())),
        part("Shifter", || Box::new(Shifter::default())),
        part("Bit Adder", || Box::new(BitAdder::default())),
        part("Bit Finder", || Box::new(BitFinder::default())),
    ];
    parts.extend(floating::parts());
    Library { name: "Arithmetic", parts }
}

// ---- shared helpers ---------------------------------------------------------------------

/// Delay in ticks (never below the simulator minimum of 1).
pub(crate) fn delay(n: usize) -> u32 {
    (n as u32 * PER_DELAY).max(1)
}

/// True if the value is absent (Java `Value.NIL`) or entirely unknown (nothing drives it).
pub(crate) fn is_absent(v: Value) -> bool {
    v == Value::NIL || v.is_unknown()
}

/// The 0/1 bits of `v` as an integer, treating X and E bits as 0.
pub(crate) fn raw(v: &Value) -> u64 {
    v.bits().iter().enumerate().fold(0, |acc, (i, b)| acc | (u64::from(*b == Value::TRUE) << i))
}

/// Sign-extend the low `w` bits of `x`.
pub(crate) fn sext(x: u64, w: u8) -> i64 {
    if w == 0 || w >= 64 {
        x as i64
    } else {
        ((x << (64 - w)) as i64) >> (64 - w)
    }
}

/// The error/unknown fill for results computed from undefined operands.
pub(crate) fn undefined_like(w: BitWidth, any_error: bool) -> Value {
    if any_error {
        Value::error(w)
    } else {
        Value::unknown(w)
    }
}

/// Two's-complement negation on a bit array that may contain X/E (shared by Negator/Absolute).
pub(crate) fn negate_bits(bits: &mut [Value], invert_tail: bool) {
    let mut fill = Value::FALSE;
    let mut pos = 0;
    while pos < bits.len() {
        match bits[pos] {
            Value::FALSE => bits[pos] = fill,
            Value::TRUE => {
                if fill != Value::FALSE {
                    bits[pos] = fill;
                }
                pos += 1;
                break;
            }
            Value::ERROR => fill = Value::ERROR,
            _ => {
                if fill == Value::FALSE {
                    fill = bits[pos];
                } else {
                    bits[pos] = fill;
                }
            }
        }
        pos += 1;
    }
    if invert_tail {
        for b in &mut bits[pos..] {
            if *b == Value::TRUE {
                *b = Value::FALSE;
            } else if *b == Value::FALSE {
                *b = Value::TRUE;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use crate::wiring::Pin;
    use logisim_core::{Circuit, CompId, Component, Direction, Location, Value};

    /// Place `comp`, drive each `Some` input port with a pin, settle, and return the circuit.
    pub fn build(comp: impl Component + 'static, inputs: &[(usize, Value)]) -> (Circuit, CompId) {
        let mut c = Circuit::new();
        let id = c.add_component(Location::new(200, 200), Direction::East, Box::new(comp));
        for &(port, v) in inputs {
            let at = c.port_location(id, port);
            c.add_component(at, Direction::East, Box::new(Pin::new(v)));
        }
        c.reset();
        (c, id)
    }

    /// Output values of the given ports after driving `inputs`.
    pub fn eval(comp: impl Component + 'static, inputs: &[(usize, Value)], outs: &[usize]) -> Vec<Value> {
        let (mut c, id) = build(comp, inputs);
        outs.iter().map(|&p| c.port_value(id, p)).collect()
    }

    pub fn k(w: u8, v: u64) -> Value {
        Value::known(logisim_core::BitWidth::new(w), v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_core::OptionMap;

    #[test]
    fn every_part_round_trips_its_options() {
        for p in &library().parts {
            let part = (p.make)();
            let saved = part.options();
            let mut fresh = (p.make)();
            fresh.set_options(&saved).unwrap_or_else(|e| panic!("{}: {e}", p.name));
            assert_eq!(fresh.options(), saved, "{}", p.name);
            assert_eq!(fresh.ports(), part.ports(), "{}", p.name);
            assert_eq!(part.name(), p.name);
            assert!(!saved.is_empty() || p.name.is_empty(), "{} should save its options", p.name);
        }
    }

    #[test]
    fn options_are_applied_and_validated() {
        let mut a = Adder::default();
        a.set_options(&OptionMap::new().with("width", 16)).unwrap();
        assert_eq!(a.ports()[0].width.get(), 16);
        assert!(a.set_options(&OptionMap::new().with("width", 65)).is_err());
        assert!(a.set_options(&OptionMap::new().with("wdith", 4)).is_err());
        assert!(a.set_options(&OptionMap::new().with("width", "8")).is_err());
        assert_eq!(a.ports()[0].width.get(), 16, "a failed set_options changes nothing");

        let mut m = Multiplier::default();
        m.set_options(&OptionMap::new().with("mode", "signed_unsigned").with("width", 4)).unwrap();
        assert_eq!(m.options().string("mode").unwrap(), Some("signed_unsigned"));
        assert!(m.set_options(&OptionMap::new().with("mode", "bogus")).is_err());

        let mut s = Shifter::default();
        s.set_options(&OptionMap::new().with("shift", "roll_right").with("width", 32)).unwrap();
        assert_eq!(s.ports()[2].width.get(), 5);

        let mut b = BitAdder::default();
        b.set_options(&OptionMap::new().with("inputs", 5)).unwrap();
        assert_eq!(b.ports().len(), 6);
        assert!(b.set_options(&OptionMap::new().with("inputs", 0)).is_err());

        let mut f = floating::FpCalc::multiplier(32, floating::FpMulMode::Multiply);
        f.set_options(&OptionMap::new().with("mode", "fused_multiply_add").with("width", 16)).unwrap();
        assert_eq!(f.ports().len(), 5);
        assert!(f.set_options(&OptionMap::new().with("width", 12)).is_err());
        let mut c = floating::FpConstant::default();
        c.set_options(&OptionMap::new().with("value", "1.5").with("width", 64)).unwrap();
        assert_eq!(c.options().string("value").unwrap(), Some("1.5"));
        assert!(c.set_options(&OptionMap::new().with("value", "abc")).is_err());
    }

    #[test]
    fn non_default_options_round_trip() {
        use crate::arith::floating::*;
        let parts: Vec<Box<dyn Component>> = vec![
            Box::new(Comparator::new(SignOptions::default().width(5).unsigned())),
            Box::new(MinMax::new(SignOptions::default().width(12).signed())),
            Box::new(Divider::new(SignOptions::default().width(3).signed())),
            Box::new(Multiplier::new(MultiplierOptions::default().width(9).mode(MulMode::Signed))),
            Box::new(Shifter::new(ShifterOptions::default().width(20).kind(ShiftKind::ArithmeticRight))),
            Box::new(BitAdder::new(BitAdderOptions::default().width(3).inputs(7))),
            Box::new(BitFinder::new(BitFinderOptions::default().width(13).find(BitFinderType::HighZero))),
            Box::new(FpCalc::logarithm(16, LogMode::Base10)),
            Box::new(FpCalc::trigonometry(64, TrigMode::Hyperbolic)),
            Box::new(FpCalc::round(8, RoundMode::Truncate)),
            Box::new(FpCalc::exponentiator(32, ExpMode::ExpMinusOne)),
            Box::new(FpToInt::new(12, 16, RoundMode::Rint)),
            Box::new(IntToFp::new(5, 64, Sign::Unsigned)),
            Box::new(FpToFp::new(8, 64)),
            Box::new(FpConstant::new(16, -0.15625)),
        ];
        for part in parts {
            let saved = part.options();
            let mut again = library().parts.iter().find(|p| p.name == part.name()).map(|p| (p.make)()).unwrap();
            again.set_options(&saved).unwrap();
            assert_eq!(again.options(), saved, "{}", part.name());
            assert_eq!(again.ports(), part.ports(), "{}", part.name());
        }
    }
}
