//! Multi-bit signal values: each bit is 0, 1, X (unknown/floating) or E (error).
//! Semantics follow Logisim's `data.Value` (width 0 is `NIL`, up to 64 bits).

use std::fmt;

/// Number of bits on a wire (1..=64). Width 0 is only used by [`Value::NIL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BitWidth(pub u8);

impl BitWidth {
    pub const ONE: BitWidth = BitWidth(1);
    pub const MAX: u8 = 64;
    pub fn new(w: u8) -> Self {
        assert!(w <= Self::MAX, "bit width {w} exceeds 64");
        BitWidth(w)
    }
    pub fn get(self) -> u8 {
        self.0
    }
    pub fn mask(self) -> u64 {
        mask(self.0)
    }
}

fn mask(width: u8) -> u64 {
    if width >= 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

/// A value on a wire. Invariant: the three bit-planes are disjoint and within `width`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value {
    width: u8,
    error: u64,
    unknown: u64,
    value: u64,
}

impl Value {
    pub const NIL: Value = Value { width: 0, error: 0, unknown: 0, value: 0 };
    pub const FALSE: Value = Value { width: 1, error: 0, unknown: 0, value: 0 };
    pub const TRUE: Value = Value { width: 1, error: 0, unknown: 0, value: 1 };
    pub const UNKNOWN: Value = Value { width: 1, error: 0, unknown: 1, value: 0 };
    pub const ERROR: Value = Value { width: 1, error: 1, unknown: 0, value: 0 };

    /// Normalising constructor: masks to width; error beats unknown beats value.
    pub fn create(width: u8, error: u64, unknown: u64, value: u64) -> Value {
        assert!(width <= 64);
        let m = mask(width);
        let error = error & m;
        let unknown = unknown & m & !error;
        let value = value & m & !error & !unknown;
        Value { width, error, unknown, value }
    }
    pub fn error(w: BitWidth) -> Value {
        Value::create(w.0, u64::MAX, 0, 0)
    }
    pub fn unknown(w: BitWidth) -> Value {
        Value::create(w.0, 0, u64::MAX, 0)
    }
    pub fn known(w: BitWidth, v: u64) -> Value {
        Value::create(w.0, 0, 0, v)
    }
    /// Builds a value from per-bit values (index 0 = least significant).
    pub fn from_bits(bits: &[Value]) -> Value {
        if bits.is_empty() {
            return Value::NIL;
        }
        let (mut e, mut u, mut v) = (0, 0, 0);
        for (i, b) in bits.iter().enumerate() {
            e |= (b.error & 1) << i;
            u |= (b.unknown & 1) << i;
            v |= (b.value & 1) << i;
        }
        Value::create(bits.len() as u8, e, u, v)
    }
    /// Every bit of the given width set to the (single-bit) `base`.
    pub fn repeat(base: Value, w: BitWidth) -> Value {
        Value::from_bits(&vec![base; w.0 as usize])
    }

    pub fn width(&self) -> BitWidth {
        BitWidth(self.width)
    }
    /// Single bit `i` as a width-1 value (`ERROR` if out of range).
    pub fn bit(&self, i: u8) -> Value {
        if i >= self.width {
            return Value::ERROR;
        }
        Value::create(1, self.error >> i, self.unknown >> i, self.value >> i)
    }
    pub fn bits(&self) -> Vec<Value> {
        (0..self.width).map(|i| self.bit(i)).collect()
    }
    pub fn with_bit(&self, i: u8, b: Value) -> Value {
        let mut bits = self.bits();
        if (i as usize) < bits.len() {
            bits[i as usize] = b;
        }
        Value::from_bits(&bits)
    }
    pub fn is_error(&self) -> bool {
        self.error != 0
    }
    pub fn is_unknown(&self) -> bool {
        self.width != 0 && (self.unknown == mask(self.width))
    }
    pub fn is_fully_defined(&self) -> bool {
        self.width != 0 && self.error == 0 && self.unknown == 0
    }
    /// Numeric value if every bit is 0/1.
    pub fn to_u64(&self) -> Option<u64> {
        self.is_fully_defined().then_some(self.value)
    }

    pub fn and(self, o: Value) -> Value {
        let f0 = !self.value & !self.error & !self.unknown;
        let f1 = !o.value & !o.error & !o.unknown;
        let falses = f0 | f1;
        Value::create(
            self.width.max(o.width),
            (self.error | o.error | self.unknown | o.unknown) & !falses,
            0,
            self.value & o.value,
        )
    }
    pub fn or(self, o: Value) -> Value {
        let t0 = self.value & !self.error & !self.unknown;
        let t1 = o.value & !o.error & !o.unknown;
        let trues = t0 | t1;
        Value::create(
            self.width.max(o.width),
            (self.error | o.error | self.unknown | o.unknown) & !trues,
            0,
            self.value | o.value,
        )
    }
    pub fn xor(self, o: Value) -> Value {
        Value::create(
            self.width.max(o.width),
            self.error | o.error | self.unknown | o.unknown,
            0,
            self.value ^ o.value,
        )
    }

    /// Merge two drivers of the same wire: X yields to the other, disagreement is E.
    pub fn combine(self, o: Value) -> Value {
        if self == Value::NIL {
            return o;
        }
        if o == Value::NIL {
            return self;
        }
        let tk = !self.unknown & mask(self.width);
        let ok = !o.unknown & mask(o.width);
        let disagree = (self.value ^ o.value) & tk & ok;
        Value::create(
            self.width.max(o.width),
            self.error | o.error | disagree,
            !tk & !ok,
            self.value | o.value,
        )
    }

    /// Tri-state enable: `self` is the control. 0 → floating, 1/X → pass `data`.
    pub fn controls(self, data: Value) -> Value {
        let w = data.width;
        if self.width == 1 {
            if self == Value::FALSE {
                return Value::create(w, 0, u64::MAX, 0);
            }
            if self == Value::TRUE || self == Value::UNKNOWN {
                return data;
            }
            return Value::create(w, u64::MAX, 0, 0);
        }
        if self.width != w {
            return Value::create(w, u64::MAX, 0, 0);
        }
        let enabled = (self.value | self.unknown) & !self.error;
        let disabled = !self.value & !self.unknown & !self.error;
        Value::create(
            w,
            self.error | (data.error & !disabled),
            disabled | data.unknown,
            enabled & data.value,
        )
    }

    /// Binary string, MSB first, using `0 1 x E`.
    pub fn to_binary_string(&self) -> String {
        (0..self.width)
            .rev()
            .map(|i| match self.bit(i) {
                Value::TRUE => '1',
                Value::FALSE => '0',
                Value::UNKNOWN => 'x',
                _ => 'E',
            })
            .collect()
    }
}

impl std::ops::Not for Value {
    type Output = Value;
    fn not(self) -> Value {
        Value::create(self.width, self.error | self.unknown, 0, !self.value)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.width == 0 {
            return write!(f, "-");
        }
        write!(f, "{}", self.to_binary_string())
    }
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Value({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Value as V;

    #[test]
    fn single_bit_and_or_xor_not() {
        assert_eq!(V::TRUE.and(V::TRUE), V::TRUE);
        assert_eq!(V::FALSE.and(V::UNKNOWN), V::FALSE);
        assert_eq!(V::TRUE.and(V::UNKNOWN), V::ERROR);
        assert_eq!(V::TRUE.or(V::UNKNOWN), V::TRUE);
        assert_eq!(V::FALSE.or(V::UNKNOWN), V::ERROR);
        assert_eq!(V::TRUE.xor(V::TRUE), V::FALSE);
        assert_eq!(V::TRUE.xor(V::UNKNOWN), V::ERROR);
        assert_eq!(!V::FALSE, V::TRUE);
        assert_eq!(!V::UNKNOWN, V::ERROR);
    }
    #[test]
    fn combine_rules() {
        assert_eq!(V::UNKNOWN.combine(V::TRUE), V::TRUE);
        assert_eq!(V::TRUE.combine(V::TRUE), V::TRUE);
        assert_eq!(V::TRUE.combine(V::FALSE), V::ERROR);
        assert_eq!(V::NIL.combine(V::FALSE), V::FALSE);
        assert_eq!(V::UNKNOWN.combine(V::UNKNOWN), V::UNKNOWN);
        assert_eq!(V::ERROR.combine(V::UNKNOWN), V::ERROR);
    }
    #[test]
    fn multi_bit() {
        let w = BitWidth::new(4);
        let a = V::known(w, 0b1100);
        let b = V::known(w, 0b1010);
        assert_eq!(a.and(b).to_u64(), Some(0b1000));
        assert_eq!(a.or(b).to_u64(), Some(0b1110));
        assert_eq!(a.xor(b).to_u64(), Some(0b0110));
        assert_eq!((!a).to_u64(), Some(0b0011));
        let x = V::create(4, 0, 0b0001, 0b1100);
        assert_eq!(x.and(V::known(w, 0b0001)).to_binary_string(), "000x".replace('x', "E"));
        assert_eq!(x.and(V::known(w, 0)).to_u64(), Some(0));
        assert_eq!(x.or(V::known(w, 1)).to_u64(), Some(0b1101));
    }
    #[test]
    fn combine_bus_and_controls() {
        let w = BitWidth::new(2);
        let a = V::create(2, 0, 0b10, 0b01);
        let b = V::create(2, 0, 0b01, 0b10);
        assert_eq!(a.combine(b).to_u64(), Some(0b11));
        assert!(V::known(w, 1).combine(V::known(w, 2)).is_error());
        assert_eq!(V::FALSE.controls(V::known(w, 3)), V::unknown(w));
        assert_eq!(V::TRUE.controls(V::known(w, 3)).to_u64(), Some(3));
        assert_eq!(V::ERROR.controls(V::known(w, 3)), V::error(w));
    }
    #[test]
    fn bits_roundtrip_and_display() {
        let v = V::from_bits(&[V::TRUE, V::UNKNOWN, V::ERROR, V::FALSE]);
        assert_eq!(v.to_string(), "0Ex1");
        assert_eq!(V::from_bits(&v.bits()), v);
        assert_eq!(V::known(BitWidth::new(64), u64::MAX).to_u64(), Some(u64::MAX));
    }
}
