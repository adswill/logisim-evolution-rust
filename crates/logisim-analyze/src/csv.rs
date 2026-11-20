//! Truth tables as CSV, one column per bit, for spreadsheets and other tools.
//!
//! The first row names the columns (`A`, `B[1]`, `B[0]`, `Sum`); consecutive `Name[k]` columns
//! counting down to 0 form one bus variable. Every other row holds `0`, `1` or `x` (don't care).
//! On import the inputs come first; how many columns are inputs is worked out from the number of
//! rows (a complete table has 2^inputs of them) unless the caller says.

use crate::model::AnalyzerModel;
use crate::table::{Entry, TruthTable};
use crate::var::{is_valid_name, Var};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvError {
    /// 1-based line, 0 when not tied to a line.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for CsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.message)
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for CsvError {}

fn err<T>(line: usize, message: impl Into<String>) -> Result<T, CsvError> {
    Err(CsvError {
        line,
        message: message.into(),
    })
}

fn quote(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// One line of CSV into fields (handles `"quoted, fields"` and `""` escapes).
fn split(line: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Write every row of the table.
pub fn export(model: &AnalyzerModel) -> String {
    let mut names = model.input_bits();
    names.extend(model.output_bits());
    let mut out = names.iter().map(|n| quote(n)).collect::<Vec<_>>().join(",");
    out.push('\n');
    let t = model.table();
    for row in 0..t.row_count() {
        let mut cells: Vec<String> = (0..t.input_count())
            .map(|c| if t.input_bit(row, c) { "1" } else { "0" }.to_string())
            .collect();
        cells.extend((0..t.output_count()).map(|o| t.get(row, o).to_char().to_string()));
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// Group bit names into variables: `A[2],A[1],A[0]` is a 3-bit `A`, anything else is 1 bit.
fn group_columns(names: &[String], line: usize) -> Result<Vec<Var>, CsvError> {
    fn bit_of(n: &str) -> Option<(&str, u32)> {
        let open = n.rfind('[')?;
        let inner = n.strip_suffix(']')?.get(open + 1..)?;
        Some((&n[..open], inner.parse().ok()?))
    }
    let mut vars = vec![];
    let mut i = 0;
    while i < names.len() {
        if let Some((base, top)) = bit_of(&names[i]) {
            // a run base[top], base[top-1] ... base[0]
            let run = (0..=top as usize).take_while(|k| {
                names
                    .get(i + k)
                    .and_then(|n| bit_of(n))
                    .is_some_and(|(b, v)| b == base && v == top - *k as u32)
            });
            if run.count() == top as usize + 1 {
                if !is_valid_name(base) {
                    return err(line, format!("'{base}' is not a valid variable name"));
                }
                vars.push(Var::new(base, top + 1));
                i += top as usize + 1;
                continue;
            }
        }
        if !is_valid_name(&names[i]) {
            return err(
                line,
                format!(
                    "column '{}' is not a valid variable name (letters, digits and _, starting with a letter; a bus is written A[2], A[1], A[0])",
                    names[i]
                ),
            );
        }
        vars.push(Var::new(&names[i], 1));
        i += 1;
    }
    Ok(vars)
}

/// Read a CSV table. `input_columns` is the number of leading input columns; `None` works it out
/// from the number of data rows (which must then be a power of two).
pub fn import(text: &str, input_columns: Option<usize>) -> Result<AnalyzerModel, CsvError> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty());
    let Some((hline, header)) = lines.next() else {
        return err(0, "the file is empty");
    };
    let names = split(header);
    let rows: Vec<(usize, Vec<String>)> = lines.map(|(n, l)| (n, split(l))).collect();
    let ncols = names.len();
    if ncols < 2 {
        return err(
            hline,
            "a truth table needs at least one input and one output column",
        );
    }
    let n_in = match input_columns {
        Some(n) => n,
        None => {
            let r = rows.len();
            if r == 0 || !r.is_power_of_two() {
                return err(
                    0,
                    format!(
                        "found {r} data rows; a complete table has 2, 4, 8 … rows. Say how many columns are inputs"
                    ),
                );
            }
            r.trailing_zeros() as usize
        }
    };
    if n_in == 0 || n_in >= ncols {
        return err(
            hline,
            format!("{n_in} input columns does not fit a file with {ncols} columns"),
        );
    }
    if n_in > TruthTable::MAX_INPUTS {
        return err(
            hline,
            format!(
                "too many input columns (at most {})",
                TruthTable::MAX_INPUTS
            ),
        );
    }
    let in_vars = group_columns(&names[..n_in], hline)?;
    let out_vars = group_columns(&names[n_in..], hline)?;
    let mut model = AnalyzerModel::new();
    for v in in_vars {
        model.add_input(v).map_err(|e| CsvError {
            line: hline,
            message: e.to_string(),
        })?;
    }
    for v in out_vars {
        model.add_output(v).map_err(|e| CsvError {
            line: hline,
            message: e.to_string(),
        })?;
    }
    let mut table = model.table().clone();
    for (n, cells) in rows {
        if cells.len() != ncols {
            return err(
                n,
                format!("expected {ncols} values but found {}", cells.len()),
            );
        }
        let parse = |c: &String| -> Result<Option<bool>, CsvError> {
            match Entry::from_char(c.chars().next().unwrap_or('?')) {
                Some(_) if c.chars().count() != 1 => err(n, format!("'{c}' should be 0, 1 or x")),
                Some(Entry::Zero) => Ok(Some(false)),
                Some(Entry::One) => Ok(Some(true)),
                Some(_) => Ok(None),
                None => err(n, format!("'{c}' should be 0, 1 or x")),
            }
        };
        let pattern: Vec<Option<bool>> =
            cells[..n_in].iter().map(parse).collect::<Result<_, _>>()?;
        let outs: Vec<Option<bool>> = cells[n_in..].iter().map(parse).collect::<Result<_, _>>()?;
        for row in table.rows_matching(&pattern) {
            for (o, v) in outs.iter().enumerate() {
                table.set(row, o, v.map_or(Entry::DontCare, Entry::from_bool));
            }
        }
    }
    model.set_table(table);
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_has_one_column_per_bit() {
        let mut m = AnalyzerModel::new();
        m.add_input(Var::new("A", 2)).unwrap();
        m.add_output(Var::new("y", 1)).unwrap();
        m.set_expression_text("y", "A[1] ^ A[0]").unwrap();
        let text = export(&m);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "A[1],A[0],y");
        assert_eq!(&lines[1..], ["0,0,0", "0,1,1", "1,0,1", "1,1,0"]);
    }

    #[test]
    fn round_trip_keeps_variables_and_table() {
        let mut m = AnalyzerModel::new();
        m.add_input(Var::new("A", 2)).unwrap();
        m.add_input(Var::new("en", 1)).unwrap();
        m.add_output(Var::new("Q", 2)).unwrap();
        m.add_output(Var::new("z", 1)).unwrap();
        let mut t = m.table().clone();
        for row in 0..8 {
            for o in 0..3 {
                t.set(
                    row,
                    o,
                    [Entry::Zero, Entry::One, Entry::DontCare][(row + 2 * o) % 3],
                );
            }
        }
        m.set_table(t);
        let back = import(&export(&m), None).unwrap();
        assert_eq!(back.inputs(), m.inputs());
        assert_eq!(back.outputs(), m.outputs());
        assert_eq!(back.table(), m.table());
    }

    #[test]
    fn quoted_names_and_crlf() {
        let text = "\"a\",\"b\",\"y\"\r\n0,0,0\r\n0,1,1\r\n1,0,1\r\n1,1,1\r\n";
        let m = import(text, None).unwrap();
        assert_eq!(m.input_bits(), ["a", "b"]);
        assert_eq!(m.expression(0).to_string(), "a + b");
    }

    #[test]
    fn explicit_input_count_with_dont_care_rows() {
        let m = import("a,b,y\n1,x,1\n0,0,0\n", Some(2)).unwrap();
        assert_eq!(m.table().get(0b10, 0), Entry::One);
        assert_eq!(m.table().get(0b11, 0), Entry::One);
        assert_eq!(m.table().get(0b00, 0), Entry::Zero);
        assert_eq!(m.table().get(0b01, 0), Entry::DontCare);
    }

    #[test]
    fn problems_name_their_line() {
        let cases: [(&str, Option<usize>, usize, &str); 7] = [
            ("", None, 0, "empty"),
            ("a\n0\n", None, 1, "at least one input"),
            ("a,b\n0,0\n0,1\n1,0\n", None, 0, "3 data rows"),
            ("a,y\n0,0\n1,2\n", None, 3, "0, 1 or x"),
            ("a,y\n0,0\n1\n", None, 3, "expected 2"),
            ("1a,y\n0,0\n1,1\n", None, 1, "not a valid"),
            ("a,y\n0,0\n1,1\n", Some(2), 1, "does not fit"),
        ];
        for (text, n, line, what) in cases {
            let e = import(text, n).unwrap_err();
            assert_eq!(e.line, line, "{text:?}: {e}");
            assert!(e.message.contains(what), "{text:?}: {e}");
        }
    }

    #[test]
    fn bus_columns_group_only_when_complete() {
        let m = import("A[1],A[0],B[1],y\n0,0,0,0\n0,0,1,0\n0,1,0,0\n0,1,1,0\n1,0,0,0\n1,0,1,0\n1,1,0,0\n1,1,1,1\n", Some(4));
        assert!(m.is_err()); // B[1] alone is not a run down to 0 and its name has a bracket
    }
}
