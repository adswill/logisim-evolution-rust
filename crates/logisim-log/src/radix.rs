//! Value formatting and parsing (port of `RadixOption` and `Value.fromLogString`).

use logisim_core::{BitWidth, Value};

/// How a signal's value is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Radix {
    #[default]
    Binary,
    Octal,
    DecimalSigned,
    DecimalUnsigned,
    Hex,
    /// IEEE float (8, 16, 32 or 64 bits).
    Float,
}

const RADIXES: [(&str, Radix); 6] = [
    ("2", Radix::Binary),
    ("8", Radix::Octal),
    ("10signed", Radix::DecimalSigned),
    ("10unsigned", Radix::DecimalUnsigned),
    ("16", Radix::Hex),
    ("float", Radix::Float),
];

pub const TRUE_CHAR: char = '1';
pub const FALSE_CHAR: char = '0';
pub const UNKNOWN_CHAR: char = 'x';
pub const ERROR_CHAR: char = 'E';
pub const DONT_CARE_CHAR: char = '-';

fn bit_char(b: Value) -> char {
    if b == Value::TRUE {
        TRUE_CHAR
    } else if b == Value::FALSE {
        FALSE_CHAR
    } else if b == Value::UNKNOWN {
        UNKNOWN_CHAR
    } else {
        ERROR_CHAR
    }
}

impl Radix {
    pub const ALL: [Radix; 6] = [
        Radix::Binary,
        Radix::Octal,
        Radix::DecimalSigned,
        Radix::DecimalUnsigned,
        Radix::Hex,
        Radix::Float,
    ];

    /// Name used in saved files (`"2"`, `"8"`, `"10signed"`, `"10unsigned"`, `"16"`, `"float"`).
    pub fn save_name(self) -> &'static str {
        RADIXES
            .iter()
            .find(|(_, r)| *r == self)
            .map_or("2", |(n, _)| n)
    }

    /// Inverse of [`save_name`](Self::save_name); unknown names give binary (as in Logisim).
    pub fn from_save_name(s: &str) -> Radix {
        RADIXES
            .iter()
            .find(|(n, _)| *n == s)
            .map_or(Radix::Binary, |(_, r)| *r)
    }

    /// One-letter suffix used in labels (`b`, `o`, `s`, `u`, `h`, `f`).
    pub fn index_char(self) -> &'static str {
        match self {
            Radix::Binary => "b",
            Radix::Octal => "o",
            Radix::DecimalSigned => "s",
            Radix::DecimalUnsigned => "u",
            Radix::Hex => "h",
            Radix::Float => "f",
        }
    }

    /// Maximum number of characters a value of this width needs.
    pub fn max_length(self, width: BitWidth) -> usize {
        let w = width.get() as usize;
        match self {
            Radix::Binary => {
                if w <= 1 {
                    1
                } else {
                    w + (w - 1) / 4
                }
            }
            Radix::Octal => w.div_ceil(3).max(1),
            Radix::Hex => w.div_ceil(4).max(1),
            Radix::DecimalSigned => {
                decimal_digits((1u128 << w.max(1).saturating_sub(1)) as f64, true)
            }
            Radix::DecimalUnsigned => decimal_digits((2f64).powi(w as i32), false),
            Radix::Float => {
                if w == 64 {
                    24
                } else {
                    12
                }
            }
        }
    }

    /// Format `v` in this radix.
    pub fn format(self, v: &Value) -> String {
        match self {
            Radix::Binary => binary_display(v),
            Radix::Octal => digit_display(v, 3),
            Radix::Hex => digit_display(v, 4),
            Radix::DecimalSigned => decimal(v, true),
            Radix::DecimalUnsigned => decimal(v, false),
            Radix::Float => float_display(v),
        }
    }
}

/// Digits of the largest magnitude of a `2^bits` range (plus a minus sign when signed).
fn decimal_digits(range: f64, signed: bool) -> usize {
    let max = if signed { range } else { range - 1.0 };
    let digits = format!("{max:.0}").len();
    if signed { digits + 1 } else { digits }.max(1)
}

/// Binary text, MSB first, a space after every 4 bits (`0101 1010`); single bits are just `0`/`1`/`x`/`E`.
pub fn binary_display(v: &Value) -> String {
    let w = v.width().get();
    match w {
        0 => DONT_CARE_CHAR.to_string(),
        1 => bit_char(v.bit(0)).to_string(),
        _ => {
            let mut s = String::new();
            for i in (0..w).rev() {
                s.push(bit_char(v.bit(i)));
                if i % 4 == 0 && i != 0 {
                    s.push(' ');
                }
            }
            s
        }
    }
}

/// Octal (`bits_per_digit` = 3) or hex (4) digits; a digit with an error bit shows `E`, with an unknown bit `x`.
fn digit_display(v: &Value, bits_per_digit: usize) -> String {
    let w = v.width().get() as usize;
    if w <= 1 {
        return binary_display(v);
    }
    let bits = v.bits();
    let digits = w.div_ceil(bits_per_digit);
    (0..digits)
        .rev()
        .map(|k| {
            let (first, last) = (bits_per_digit * k, w.min(bits_per_digit * (k + 1)));
            let mut acc = 0u32;
            for b in bits[first..last].iter().rev() {
                if *b == Value::ERROR {
                    return ERROR_CHAR;
                }
                if *b == Value::UNKNOWN {
                    return UNKNOWN_CHAR;
                }
                acc = acc * 2 + u32::from(*b == Value::TRUE);
            }
            char::from_digit(acc, 16).unwrap_or('?')
        })
        .collect()
}

fn decimal(v: &Value, signed: bool) -> String {
    let w = v.width().get();
    if w == 0 {
        return DONT_CARE_CHAR.to_string();
    }
    if v.is_error() {
        return ERROR_CHAR.to_string();
    }
    let Some(raw) = v.to_u64() else {
        return UNKNOWN_CHAR.to_string();
    };
    if signed && w < 64 && (raw >> (w - 1)) & 1 == 1 {
        return (raw as i64 - (1i64 << w)).to_string();
    }
    if signed {
        (raw as i64).to_string()
    } else {
        raw.to_string()
    }
}

/// Decode a minifloat (`exp_bits`/`man_bits`) to `f64`.
fn small_float(bits: u64, exp_bits: u32, man_bits: u32) -> f64 {
    let bias = (1i32 << (exp_bits - 1)) - 1;
    let max_exp = (1u64 << exp_bits) - 1;
    let sign = if (bits >> (exp_bits + man_bits)) & 1 == 1 {
        -1.0
    } else {
        1.0
    };
    let exp = (bits >> man_bits) & max_exp;
    let man = bits & ((1u64 << man_bits) - 1);
    if exp == 0 {
        sign * man as f64 * 2f64.powi(1 - bias - man_bits as i32)
    } else if exp == max_exp {
        if man == 0 {
            sign * f64::INFINITY
        } else {
            f64::NAN
        }
    } else {
        sign * (1.0 + man as f64 / (1u64 << man_bits) as f64) * 2f64.powi(exp as i32 - bias)
    }
}

/// Java-style float text: `1.0`, `0.5`, `NaN`, `Infinity`.
fn java_float_text(x: f64, single: bool) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else if single {
        format!("{:?}", x as f32)
    } else {
        format!("{x:?}")
    }
}

fn float_display(v: &Value) -> String {
    let w = v.width().get();
    let Some(raw) = v.to_u64() else {
        return "NaN".into();
    };
    match w {
        8 => java_float_text(small_float(raw, 4, 3), true),
        16 => java_float_text(small_float(raw, 5, 10), true),
        32 => java_float_text(f64::from(f32::from_bits(raw as u32)), true),
        64 => java_float_text(f64::from_bits(raw), false),
        _ => "NaN".into(),
    }
}

// ---- log strings (test vectors, saved logs) -------------------------------------------------

/// Radix (2, 8, 10 or 16) a log string is written in: `0x`, `0o`, `0b` prefixes, a length equal
/// to the width means binary, anything else is decimal.
pub fn radix_of_log_string(width: BitWidth, t: &str) -> u32 {
    if t.starts_with("0x") {
        16
    } else if t.starts_with("0o") {
        8
    } else if t.starts_with("0b") || t.len() == width.get() as usize {
        2
    } else {
        10
    }
}

fn too_many_bits(t: &str, w: u8, suggested: u32, reminder: &str) -> String {
    let plural = if w != 1 { "s" } else { "" };
    let hint = if suggested > 0 {
        format!(" did you mean [{suggested}]?")
    } else {
        String::new()
    };
    format!("Too many bits in \"{t}\" expected {w} bit{plural}{hint}{reminder}")
}

/// Parse a value written as in test vectors and log files: binary (`0101`, `0b0101`), octal
/// (`0o17`), hex (`0xFF`) or decimal (`-5`), with `_` separators and `x` for unknown digits
/// (in non-decimal radixes). Errors say what is wrong and suggest the width when it is too small.
pub fn parse_log_string(width: BitWidth, t: &str) -> Result<Value, String> {
    let cleaned: String = t.chars().filter(|c| *c != '_').collect();
    let radix = radix_of_log_string(width, &cleaned);
    let offset = match radix {
        16 if cleaned.starts_with("0x") => 2,
        8 if cleaned.starts_with("0o") => 2,
        2 if cleaned.starts_with("0b") => 2,
        10 if cleaned.starts_with('-') => 1,
        _ => 0,
    };
    let chars: Vec<char> = cleaned.chars().collect();
    if chars.len() <= offset {
        return Err("expected digits".into());
    }
    let w = width.get();
    let (mut value, mut unknown) = (0i128, 0u64);
    for &c in &chars[offset..] {
        let d: i32 = match c {
            'x' if radix != 10 => -1,
            '0'..='9' => c as i32 - '0' as i32,
            'a'..='f' => 10 + (c as i32 - 'a' as i32),
            'A'..='F' => 10 + (c as i32 - 'A' as i32),
            _ => return Err(format!("Unexpected character '{c}' in \"{t}\"")),
        };
        if d >= radix as i32 {
            return Err(format!("Unexpected character '{c}' in \"{t}\""));
        }
        value = value.saturating_mul(i128::from(radix));
        unknown = unknown.wrapping_mul(u64::from(radix));
        if radix != 10 {
            if d == -1 {
                unknown |= u64::from(radix - 1);
            } else {
                value |= i128::from(d);
            }
        } else if d == -1 {
            unknown += u64::from(radix - 1);
        } else {
            value += i128::from(d);
        }
    }
    if radix == 10 && cleaned.starts_with('-') {
        value = -value;
    }
    let bits_of = |n: i128| 128 - n.unsigned_abs().leading_zeros();
    if radix == 10 {
        let max = (1i128 << (w - 1)) - 1;
        let min = -(1i128 << (w - 1));
        if value > max || value < min {
            let need = if value == 0 { 1 } else { bits_of(value) + 1 };
            return Err(too_many_bits(t, w, need, ""));
        }
    } else if (value >> w) != 0 {
        let (suggested, reminder) = match radix {
            16 if cleaned.len() > 2 => (
                (cleaned.len() as u32 - 2) * 4,
                " Remember that 0x means hex and each hex digit is 4 bits",
            ),
            2 if !cleaned.is_empty() => (
                cleaned.len() as u32 - if cleaned.starts_with("0b") { 2 } else { 0 },
                " Remember that 0b means binary and each binary digit is 1 bit",
            ),
            8 if cleaned.len() > 2 => (
                (cleaned.len() as u32 - 2) * 3,
                " Remember that 0o means octal and each octal digit is 3 bits",
            ),
            _ => (bits_of(value).max(1), ""),
        };
        return Err(too_many_bits(t, w, suggested, reminder));
    }
    let mask = if w >= 64 { u64::MAX } else { (1u64 << w) - 1 };
    Ok(Value::create(w, 0, unknown & mask, value as u64 & mask))
}

/// True if every bit `expected` defines is matched by `actual` (Java `Value.compatible`):
/// an unknown expected bit matches anything; an expected error needs an error; 0/1 must match exactly.
pub fn compatible(expected: &Value, actual: &Value) -> bool {
    expected.width() == actual.width()
        && (0..expected.width().get()).all(|i| {
            let (e, a) = (expected.bit(i), actual.bit(i));
            e == Value::UNKNOWN || e == a
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(n: u8) -> BitWidth {
        BitWidth::new(n)
    }

    #[test]
    fn display_in_every_radix() {
        let v = Value::known(w(8), 0xA5);
        assert_eq!(Radix::Binary.format(&v), "1010 0101");
        assert_eq!(Radix::Octal.format(&v), "245");
        assert_eq!(Radix::Hex.format(&v), "a5");
        assert_eq!(Radix::DecimalUnsigned.format(&v), "165");
        assert_eq!(Radix::DecimalSigned.format(&v), "-91");
        assert_eq!(Radix::Binary.format(&Value::TRUE), "1");
        let part = Value::create(8, 0, 0x0F, 0xA0);
        assert_eq!(Radix::Hex.format(&part), "ax");
        assert_eq!(Radix::DecimalUnsigned.format(&part), "x");
        assert_eq!(Radix::Hex.format(&Value::create(8, 0x0F, 0, 0xA0)), "aE");
        assert_eq!(
            Radix::DecimalSigned.format(&Value::known(w(64), u64::MAX)),
            "-1"
        );
        assert_eq!(Radix::Binary.format(&Value::NIL), "-");
    }

    #[test]
    fn float_display_and_widths() {
        assert_eq!(
            Radix::Float.format(&Value::known(w(32), 0x3FC0_0000)),
            "1.5"
        );
        assert_eq!(Radix::Float.format(&Value::known(w(16), 0x3C00)), "1.0");
        assert_eq!(Radix::Float.format(&Value::known(w(8), 0x38)), "1.0");
        assert_eq!(
            Radix::Float.format(&Value::known(w(64), f64::INFINITY.to_bits())),
            "Infinity"
        );
        assert_eq!(
            Radix::Float.format(&Value::known(w(32), 0x7FC0_0000)),
            "NaN"
        );
        assert_eq!(Radix::Binary.max_length(w(8)), 9);
        assert_eq!(Radix::Hex.max_length(w(9)), 3);
        assert_eq!(Radix::DecimalUnsigned.max_length(w(8)), 3);
        assert_eq!(Radix::DecimalSigned.max_length(w(8)), 4);
        assert_eq!(Radix::DecimalUnsigned.max_length(w(64)), 20);
    }

    #[test]
    fn save_names_round_trip() {
        for r in Radix::ALL {
            assert_eq!(Radix::from_save_name(r.save_name()), r);
        }
        assert_eq!(Radix::from_save_name("nonsense"), Radix::Binary);
    }

    #[test]
    fn parse_log_strings() {
        assert_eq!(
            parse_log_string(w(4), "1010").unwrap(),
            Value::known(w(4), 10)
        );
        assert_eq!(
            parse_log_string(w(8), "0xFF").unwrap(),
            Value::known(w(8), 255)
        );
        assert_eq!(
            parse_log_string(w(8), "0b1010_0101").unwrap(),
            Value::known(w(8), 0xA5)
        );
        assert_eq!(
            parse_log_string(w(8), "0o17").unwrap(),
            Value::known(w(8), 15)
        );
        assert_eq!(
            parse_log_string(w(8), "-1").unwrap(),
            Value::known(w(8), 255)
        );
        assert_eq!(
            parse_log_string(w(8), "100").unwrap(),
            Value::known(w(8), 100)
        );
        assert_eq!(
            parse_log_string(w(8), "0x0x").unwrap(),
            Value::create(8, 0, 0x0F, 0)
        );
        assert_eq!(
            parse_log_string(w(4), "01x0").unwrap(),
            Value::create(4, 0, 0b0010, 0b0100)
        );
        assert_eq!(parse_log_string(w(1), "1").unwrap(), Value::TRUE);
        assert_eq!(
            parse_log_string(w(64), "0xFFFF_FFFF_FFFF_FFFF").unwrap(),
            Value::known(w(64), u64::MAX)
        );
    }

    #[test]
    fn parse_errors_are_helpful() {
        assert_eq!(parse_log_string(w(4), "0x").unwrap_err(), "expected digits");
        assert!(parse_log_string(w(4), "0xZ")
            .unwrap_err()
            .contains("Unexpected character 'Z'"));
        assert!(parse_log_string(w(4), "0b12")
            .unwrap_err()
            .contains("Unexpected character '2'"));
        let e = parse_log_string(w(4), "0xFF").unwrap_err();
        assert!(
            e.contains("expected 4 bits") && e.contains("did you mean [8]?"),
            "{e}"
        );
        let e = parse_log_string(w(8), "300").unwrap_err();
        assert!(e.contains("did you mean [10]?"), "{e}");
        assert!(parse_log_string(w(8), "128").is_err());
        assert!(parse_log_string(w(8), "-129").is_err());
        assert!(parse_log_string(w(8), "-128").is_ok());
    }

    #[test]
    fn compatibility_rules() {
        let x = Value::create(4, 0, 0b0011, 0b1000);
        assert!(compatible(&x, &Value::known(w(4), 0b1011)));
        assert!(compatible(&x, &Value::create(4, 0, 0b0001, 0b1000)));
        assert!(!compatible(&x, &Value::known(w(4), 0b0011)));
        assert!(compatible(&Value::error(w(2)), &Value::error(w(2))));
        assert!(!compatible(&Value::error(w(2)), &Value::known(w(2), 0)));
        assert!(!compatible(&Value::known(w(2), 0), &Value::unknown(w(2))));
        assert!(!compatible(&Value::TRUE, &Value::known(w(2), 1)));
    }
}
