//! Parsing and showing integers in decimal, hex (0x..) and binary (0b..).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Base {
    #[default]
    Dec,
    Hex,
    Bin,
}

impl Base {
    pub const ALL: [Base; 3] = [Base::Dec, Base::Hex, Base::Bin];
    pub fn label(self) -> &'static str {
        match self {
            Base::Dec => "dec",
            Base::Hex => "hex",
            Base::Bin => "bin",
        }
    }
}

/// Parse `42`, `-7`, `0x1F`, `0b1010`, `0o17` (underscores allowed). Values up to `u64::MAX`
/// are accepted and wrap into `i64`, which is how masks and wide constants are stored.
pub fn parse_int(text: &str) -> Result<i64, String> {
    let t: String = text.trim().chars().filter(|c| *c != '_').collect();
    if t.is_empty() {
        return Err("enter a number".into());
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.as_str()),
    };
    let lower = body.to_ascii_lowercase();
    let (radix, digits) = if let Some(d) = lower.strip_prefix("0x") {
        (16, d)
    } else if let Some(d) = lower.strip_prefix("0b") {
        (2, d)
    } else if let Some(d) = lower.strip_prefix("0o") {
        (8, d)
    } else {
        (10, lower.as_str())
    };
    if digits.is_empty() {
        return Err(format!("'{text}' has no digits after the prefix"));
    }
    let v = u64::from_str_radix(digits, radix).map_err(|_| match radix {
        2 => format!("'{text}' is not a binary number (use only 0 and 1)"),
        16 => format!("'{text}' is not a hex number (use 0-9 and A-F)"),
        8 => format!("'{text}' is not an octal number"),
        _ => format!("'{text}' is not a number (try 12, 0x1F or 0b1010)"),
    })?;
    if neg {
        i64::try_from(v)
            .map(|v| -v)
            .map_err(|_| format!("'{text}' is too small"))
    } else {
        Ok(v as i64)
    }
}

pub fn format_int(v: i64, base: Base) -> String {
    match base {
        Base::Dec => v.to_string(),
        Base::Hex => format!("0x{:X}", v as u64),
        Base::Bin => format!("0b{:b}", v as u64),
    }
}

/// `31 = 0x1F = 0b11111`, skipping the base that is already shown.
pub fn other_bases(v: i64, shown: Base) -> String {
    Base::ALL
        .iter()
        .filter(|b| **b != shown)
        .map(|b| format_int(v, *b))
        .collect::<Vec<_>>()
        .join("  =  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_bases() {
        assert_eq!(parse_int("42"), Ok(42));
        assert_eq!(parse_int(" -7 "), Ok(-7));
        assert_eq!(parse_int("0x1F"), Ok(31));
        assert_eq!(parse_int("0XfF"), Ok(255));
        assert_eq!(parse_int("0b1010"), Ok(10));
        assert_eq!(parse_int("0b1010_1010"), Ok(170));
        assert_eq!(parse_int("0o17"), Ok(15));
        assert_eq!(parse_int("0xFFFFFFFFFFFFFFFF"), Ok(-1)); // u64::MAX wraps
    }

    #[test]
    fn explains_bad_input() {
        assert!(parse_int("").unwrap_err().contains("enter"));
        assert!(parse_int("0b102").unwrap_err().contains("binary"));
        assert!(parse_int("0xZZ").unwrap_err().contains("hex"));
        assert!(parse_int("12ab").unwrap_err().contains("not a number"));
        assert!(parse_int("0x").unwrap_err().contains("digits"));
        assert!(parse_int("99999999999999999999").is_err());
    }

    #[test]
    fn formats() {
        assert_eq!(format_int(31, Base::Hex), "0x1F");
        assert_eq!(format_int(5, Base::Bin), "0b101");
        assert_eq!(format_int(-1, Base::Hex), "0xFFFFFFFFFFFFFFFF");
        assert_eq!(other_bases(5, Base::Dec), "0x5  =  0b101");
        assert_eq!(parse_int(&format_int(255, Base::Bin)), Ok(255));
    }
}
