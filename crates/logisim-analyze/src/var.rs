//! Named variables (1 to 32 bits wide) and ordered lists of them.

use std::fmt;

/// A named input or output of the analysed circuit; wider than one bit means a bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Var {
    pub name: String,
    pub width: u32,
}

/// Why a variable name or a list change was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarError {
    /// The text is not `Name` or `Name[N..0]`.
    Format(String),
    TooWide {
        name: String,
        max: u32,
    },
    Empty,
    Duplicate(String),
    /// Adding this would exceed the list's bit capacity.
    Capacity {
        max: usize,
    },
    NotFound(String),
}

impl fmt::Display for VarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VarError::Format(t) => write!(
                f,
                "'{t}' is not a valid variable: write a name such as A, or a bus such as Data[7..0]"
            ),
            VarError::TooWide { name, max } => {
                write!(f, "'{name}' has too many bits (at most {max} per variable)")
            }
            VarError::Empty => write!(f, "a variable needs a name"),
            VarError::Duplicate(n) => write!(f, "there is already a variable named '{n}'"),
            VarError::Capacity { max } => write!(f, "too many bits: at most {max} are allowed"),
            VarError::NotFound(n) => write!(f, "there is no variable named '{n}'"),
        }
    }
}

impl std::error::Error for VarError {}

pub const MAX_VAR_WIDTH: u32 = 32;

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}
fn is_ident_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// True when `s` can be used as a variable name in expressions.
pub fn is_valid_name(s: &str) -> bool {
    let mut it = s.chars();
    matches!(it.next(), Some(c) if is_ident_start(c)) && it.all(is_ident_part)
}

impl Var {
    pub fn new(name: &str, width: u32) -> Self {
        Var {
            name: name.to_string(),
            width,
        }
    }

    /// Name of bit `b` (0 = least significant): `A` for a 1-bit variable, `A[3]` otherwise.
    pub fn bit_name(&self, b: u32) -> String {
        assert!(b < self.width, "bit {b} of a {}-bit variable", self.width);
        if self.width > 1 {
            format!("{}[{b}]", self.name)
        } else {
            self.name.clone()
        }
    }

    /// The bit names, most significant first (the order of table columns).
    pub fn bit_names(&self) -> Vec<String> {
        (0..self.width).rev().map(|b| self.bit_name(b)).collect()
    }

    /// Parse `Name` or `Name[N..0]` (a bus of N+1 bits).
    pub fn parse(text: &str) -> Result<Var, VarError> {
        let s = text.trim();
        let (name, width) = match (s.find('['), s.rfind(']')) {
            (Some(i), Some(j)) if i > 0 && i < j && j == s.len() - 1 => {
                let inner = &s[i + 1..j];
                let top = inner
                    .strip_suffix("..0")
                    .and_then(|n| n.trim().parse::<u32>().ok())
                    .ok_or_else(|| VarError::Format(s.to_string()))?;
                (s[..i].trim(), top + 1)
            }
            (None, None) => (s, 1),
            _ => return Err(VarError::Format(s.to_string())),
        };
        if name.is_empty() {
            return Err(VarError::Empty);
        }
        if !is_valid_name(name) {
            return Err(VarError::Format(s.to_string()));
        }
        if width > MAX_VAR_WIDTH {
            return Err(VarError::TooWide {
                name: name.to_string(),
                max: MAX_VAR_WIDTH,
            });
        }
        Ok(Var::new(name, width))
    }
}

impl fmt::Display for Var {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.width > 1 {
            write!(f, "{}[{}..0]", self.name, self.width - 1)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

/// An ordered list of variables with a limit on the total number of bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarList {
    vars: Vec<Var>,
    max_bits: usize,
}

impl VarList {
    pub fn new(max_bits: usize) -> Self {
        VarList {
            vars: vec![],
            max_bits,
        }
    }

    pub fn vars(&self) -> &[Var] {
        &self.vars
    }
    pub fn max_bits(&self) -> usize {
        self.max_bits
    }
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }
    pub fn bit_count(&self) -> usize {
        self.vars.iter().map(|v| v.width as usize).sum()
    }
    /// Every bit name in column order (variables in order, each most significant bit first).
    pub fn bits(&self) -> Vec<String> {
        self.vars.iter().flat_map(|v| v.bit_names()).collect()
    }
    pub fn position(&self, name: &str) -> Option<usize> {
        self.vars.iter().position(|v| v.name == name)
    }
    /// Index of the first bit column of the variable at `index`.
    pub fn first_bit(&self, index: usize) -> usize {
        self.vars[..index].iter().map(|v| v.width as usize).sum()
    }

    fn check(&self, v: &Var, skip: Option<usize>, others: &[&VarList]) -> Result<(), VarError> {
        if v.name.is_empty() {
            return Err(VarError::Empty);
        }
        if !is_valid_name(&v.name) {
            return Err(VarError::Format(v.name.clone()));
        }
        if v.width == 0 || v.width > MAX_VAR_WIDTH {
            return Err(VarError::TooWide {
                name: v.name.clone(),
                max: MAX_VAR_WIDTH,
            });
        }
        let dup_here = self
            .vars
            .iter()
            .enumerate()
            .any(|(i, o)| Some(i) != skip && o.name == v.name);
        let dup_other = others.iter().any(|l| l.position(&v.name).is_some());
        if dup_here || dup_other {
            return Err(VarError::Duplicate(v.name.clone()));
        }
        Ok(())
    }

    /// Add a variable at the end. `others` are companion lists whose names must stay distinct
    /// from this one (inputs and outputs may not share names).
    pub fn add(&mut self, v: Var, others: &[&VarList]) -> Result<(), VarError> {
        self.check(&v, None, others)?;
        if self.bit_count() + v.width as usize > self.max_bits {
            return Err(VarError::Capacity { max: self.max_bits });
        }
        self.vars.push(v);
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<Var, VarError> {
        let i = self
            .position(name)
            .ok_or_else(|| VarError::NotFound(name.to_string()))?;
        Ok(self.vars.remove(i))
    }

    /// Replace the variable called `old` (new name and/or width), keeping its position.
    pub fn replace(&mut self, old: &str, new: Var, others: &[&VarList]) -> Result<(), VarError> {
        let i = self
            .position(old)
            .ok_or_else(|| VarError::NotFound(old.to_string()))?;
        self.check(&new, Some(i), others)?;
        let bits = self.bit_count() - self.vars[i].width as usize + new.width as usize;
        if bits > self.max_bits {
            return Err(VarError::Capacity { max: self.max_bits });
        }
        self.vars[i] = new;
        Ok(())
    }

    /// Move a variable up (`delta < 0`) or down in the list; clamped to the ends.
    pub fn move_by(&mut self, name: &str, delta: isize) -> Result<usize, VarError> {
        let i = self
            .position(name)
            .ok_or_else(|| VarError::NotFound(name.to_string()))?;
        let j = (i as isize + delta).clamp(0, self.vars.len() as isize - 1) as usize;
        let v = self.vars.remove(i);
        self.vars.insert(j, v);
        Ok(j)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display_round_trip() {
        for t in ["A", "x_1", "Data[7..0]", "Q[1..0]"] {
            assert_eq!(Var::parse(t).unwrap().to_string(), t);
        }
        assert_eq!(Var::parse(" B [3..0] ").unwrap(), Var::new("B", 4));
        assert_eq!(Var::parse("A[0..0]").unwrap().width, 1);
    }

    #[test]
    fn parse_rejects_bad_names() {
        assert!(matches!(Var::parse("A[3]"), Err(VarError::Format(_))));
        assert!(matches!(Var::parse("A[3..1]"), Err(VarError::Format(_))));
        assert!(matches!(Var::parse("[3..0]"), Err(VarError::Format(_))));
        assert!(matches!(Var::parse("1A"), Err(VarError::Format(_))));
        assert!(matches!(Var::parse("A B"), Err(VarError::Format(_))));
        assert!(matches!(Var::parse(""), Err(VarError::Empty)));
        assert!(matches!(
            Var::parse("A[40..0]"),
            Err(VarError::TooWide { .. })
        ));
    }

    #[test]
    fn bit_names_are_msb_first() {
        assert_eq!(Var::new("D", 3).bit_names(), ["D[2]", "D[1]", "D[0]"]);
        assert_eq!(Var::new("A", 1).bit_names(), ["A"]);
    }

    #[test]
    fn list_enforces_unique_names_and_capacity() {
        let mut inputs = VarList::new(4);
        let mut outputs = VarList::new(4);
        inputs.add(Var::new("A", 1), &[&outputs]).unwrap();
        inputs.add(Var::new("B", 2), &[&outputs]).unwrap();
        assert_eq!(inputs.bits(), ["A", "B[1]", "B[0]"]);
        assert_eq!(inputs.first_bit(1), 1);
        assert_eq!(
            inputs.add(Var::new("A", 1), &[&outputs]),
            Err(VarError::Duplicate("A".into()))
        );
        assert_eq!(
            inputs.add(Var::new("C", 2), &[&outputs]),
            Err(VarError::Capacity { max: 4 })
        );
        outputs.add(Var::new("Y", 1), &[&inputs]).unwrap();
        assert_eq!(
            outputs.add(Var::new("A", 1), &[&inputs]),
            Err(VarError::Duplicate("A".into()))
        );
    }

    #[test]
    fn list_replace_move_remove() {
        let mut l = VarList::new(8);
        for n in ["A", "B", "C"] {
            l.add(Var::new(n, 1), &[]).unwrap();
        }
        l.replace("B", Var::new("B", 3), &[]).unwrap();
        assert_eq!(l.bit_count(), 5);
        assert_eq!(
            l.replace("B", Var::new("C", 1), &[]),
            Err(VarError::Duplicate("C".into()))
        );
        l.replace("B", Var::new("B", 1), &[]).unwrap();
        assert_eq!(l.move_by("C", -2), Ok(0));
        assert_eq!(l.bits(), ["C", "A", "B"]);
        assert_eq!(l.move_by("C", 99), Ok(2));
        assert_eq!(l.remove("A").unwrap().name, "A");
        assert!(l.remove("A").is_err());
    }
}
