//! Library-agnostic part options: a small typed map that parts read and write so the
//! file format never needs to know about individual components.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptVal {
    Bool(bool),
    Int(i64),
    Str(String),
}

impl From<bool> for OptVal {
    fn from(v: bool) -> Self {
        OptVal::Bool(v)
    }
}
impl From<i64> for OptVal {
    fn from(v: i64) -> Self {
        OptVal::Int(v)
    }
}
impl From<i32> for OptVal {
    fn from(v: i32) -> Self {
        OptVal::Int(v as i64)
    }
}
impl From<u32> for OptVal {
    fn from(v: u32) -> Self {
        OptVal::Int(v as i64)
    }
}
impl From<u8> for OptVal {
    fn from(v: u8) -> Self {
        OptVal::Int(v as i64)
    }
}
impl From<usize> for OptVal {
    fn from(v: usize) -> Self {
        OptVal::Int(v as i64)
    }
}
impl From<&str> for OptVal {
    fn from(v: &str) -> Self {
        OptVal::Str(v.to_string())
    }
}
impl From<String> for OptVal {
    fn from(v: String) -> Self {
        OptVal::Str(v)
    }
}

/// Key → value options of one placed part. Keys are lower_snake_case.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OptionMap(pub BTreeMap<String, OptVal>);

impl OptionMap {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with(mut self, key: &str, v: impl Into<OptVal>) -> Self {
        self.0.insert(key.to_string(), v.into());
        self
    }
    pub fn set(&mut self, key: &str, v: impl Into<OptVal>) {
        self.0.insert(key.to_string(), v.into());
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Integer option in `lo..=hi`; `Ok(None)` if absent.
    pub fn int(&self, key: &str, lo: i64, hi: i64) -> Result<Option<i64>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(OptVal::Int(v)) if (lo..=hi).contains(v) => Ok(Some(*v)),
            Some(OptVal::Int(v)) => Err(format!("option '{key}' = {v} is outside {lo}..={hi}")),
            Some(_) => Err(format!("option '{key}' must be an integer")),
        }
    }
    pub fn boolean(&self, key: &str) -> Result<Option<bool>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(OptVal::Bool(v)) => Ok(Some(*v)),
            Some(_) => Err(format!("option '{key}' must be a boolean")),
        }
    }
    pub fn string(&self, key: &str) -> Result<Option<&str>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(OptVal::Str(v)) => Ok(Some(v)),
            Some(_) => Err(format!("option '{key}' must be a string")),
        }
    }
    /// String option mapped through a name table (for enums).
    pub fn choice<T: Copy>(&self, key: &str, table: &[(&str, T)]) -> Result<Option<T>, String> {
        match self.string(key)? {
            None => Ok(None),
            Some(s) => table
                .iter()
                .find(|(n, _)| *n == s)
                .map(|(_, v)| Some(*v))
                .ok_or_else(|| {
                    let names: Vec<&str> = table.iter().map(|(n, _)| *n).collect();
                    format!("option '{key}' = '{s}' is not one of {names:?}")
                }),
        }
    }
    /// Name for an enum value in `table` (the inverse of [`choice`](Self::choice)).
    pub fn name_of<T: PartialEq + Copy>(v: T, table: &[(&'static str, T)]) -> &'static str {
        table.iter().find(|(_, t)| *t == v).map_or("?", |(n, _)| n)
    }
    /// Error if any key is not in `known` (catches typos in hand-edited files).
    pub fn reject_unknown(&self, known: &[&str]) -> Result<(), String> {
        match self.0.keys().find(|k| !known.contains(&k.as_str())) {
            Some(k) => Err(format!("unknown option '{k}' (known: {known:?})")),
            None => Ok(()),
        }
    }
}

/// What kind of value an option holds, for building editor widgets and validating input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionKind {
    Bool,
    /// Integer in `min..=max` (shown in decimal, hex or binary by the editor).
    Int {
        min: i64,
        max: i64,
    },
    /// One of a fixed set of names (the same names `set_options` accepts).
    Choice {
        names: Vec<&'static str>,
    },
    Str,
}

/// Editor-facing description of one option of a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionSpec {
    pub key: &'static str,
    /// Human-readable label shown in the attribute panel.
    pub label: &'static str,
    pub kind: OptionKind,
    pub default: OptVal,
}

impl OptionSpec {
    pub fn boolean(key: &'static str, label: &'static str, default: bool) -> Self {
        OptionSpec {
            key,
            label,
            kind: OptionKind::Bool,
            default: OptVal::Bool(default),
        }
    }
    pub fn int(key: &'static str, label: &'static str, min: i64, max: i64, default: i64) -> Self {
        OptionSpec {
            key,
            label,
            kind: OptionKind::Int { min, max },
            default: OptVal::Int(default),
        }
    }
    pub fn choice(
        key: &'static str,
        label: &'static str,
        names: &[&'static str],
        default: &'static str,
    ) -> Self {
        OptionSpec {
            key,
            label,
            kind: OptionKind::Choice {
                names: names.to_vec(),
            },
            default: OptVal::Str(default.into()),
        }
    }
    pub fn string(key: &'static str, label: &'static str, default: &str) -> Self {
        OptionSpec {
            key,
            label,
            kind: OptionKind::Str,
            default: OptVal::Str(default.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_access_and_errors() {
        let o = OptionMap::new()
            .with("n", 3)
            .with("flag", true)
            .with("mode", "odd");
        assert_eq!(o.int("n", 1, 5), Ok(Some(3)));
        assert_eq!(o.int("missing", 1, 5), Ok(None));
        assert!(o.int("n", 5, 9).is_err());
        assert!(o.int("flag", 0, 1).is_err());
        assert_eq!(o.boolean("flag"), Ok(Some(true)));
        let table = [("odd", 1), ("one", 2)];
        assert_eq!(o.choice("mode", &table), Ok(Some(1)));
        assert_eq!(OptionMap::name_of(2, &table), "one");
        assert!(o.reject_unknown(&["n", "flag"]).is_err());
        assert!(o.reject_unknown(&["n", "flag", "mode"]).is_ok());
    }
}
