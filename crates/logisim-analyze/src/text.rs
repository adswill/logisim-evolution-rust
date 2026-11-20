//! Truth tables as plain text, the way Logisim Evolution exports and imports them.
//!
//! ```text
//! # Anything after a '#' is a comment.
//! A B[3..0] | D[3..0]
//! ~~~~~~~~~~~~~~~~~~~
//! 0  0000   |  1010
//! 1  ----   |  0000
//! ```
//!
//! Values are binary (`0`, `1`, and `x` or `-` for "don't care") or, when the text is shorter
//! than the variable is wide, hexadecimal with `x` standing for four don't-care bits. An input
//! row with don't cares stands for every row it matches. Rows left out stay don't care.

use crate::model::{AnalyzerModel, ModelError};
use crate::table::{Entry, TruthTable};
use crate::var::Var;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextError {
    /// 1-based line number, 0 when the problem is not tied to a line.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.message)
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for TextError {}

fn err<T>(line: usize, message: impl Into<String>) -> Result<T, TextError> {
    Err(TextError {
        line,
        message: message.into(),
    })
}

/// Write the whole table (every row) as text.
pub fn export(model: &AnalyzerModel) -> String {
    let ins = model.inputs().vars();
    let outs = model.outputs().vars();
    let widths: Vec<usize> = ins
        .iter()
        .chain(outs)
        .map(|v| v.to_string().len().max(v.width as usize))
        .collect();
    let centre = |s: &str, w: usize| {
        let pad = w.saturating_sub(s.len());
        format!("{}{}{}", " ".repeat(pad / 2), s, " ".repeat(pad - pad / 2))
    };
    let mut out = String::from(
        "# Truth table. You can edit this file and import it again.\n\
         # Anything after a '#' is a comment; blank lines and lines of ~ are ignored.\n\
         # 'Name[N..0]' is an (N+1)-bit variable, a plain 'Name' is one bit.\n\
         # Use 'x' or '-' for \"don't care\". Values are binary, or hexadecimal when shorter than the width.\n\n",
    );
    let mut header = String::new();
    for (i, v) in ins.iter().enumerate() {
        header.push_str(&centre(&v.to_string(), widths[i]));
        header.push(' ');
    }
    header.push('|');
    for (i, v) in outs.iter().enumerate() {
        header.push(' ');
        header.push_str(&centre(&v.to_string(), widths[ins.len() + i]));
    }
    let header = header.trim_end().to_string();
    out.push_str(&header);
    out.push('\n');
    out.push_str(&"~".repeat(header.len()));
    out.push('\n');
    let t = model.table();
    for row in 0..t.row_count() {
        let mut line = String::new();
        let mut col = 0;
        for (i, v) in ins.iter().enumerate() {
            let s: String = (0..v.width as usize)
                .map(|k| if t.input_bit(row, col + k) { '1' } else { '0' })
                .collect();
            col += v.width as usize;
            line.push_str(&centre(&s, widths[i]));
            line.push(' ');
        }
        line.push('|');
        let mut col = 0;
        for (i, v) in outs.iter().enumerate() {
            let s: String = (0..v.width as usize)
                .map(|k| t.get(row, col + k).to_char())
                .collect();
            col += v.width as usize;
            line.push(' ');
            line.push_str(&centre(&s, widths[ins.len() + i]));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// One value token as bit entries, most significant first. `None` entries are "don't care".
fn parse_value(tok: &str, width: usize, line: usize) -> Result<Vec<Option<bool>>, TextError> {
    let bin = |c: char| match c {
        '0' => Some(Some(false)),
        '1' => Some(Some(true)),
        'x' | 'X' | '-' => Some(None),
        _ => None,
    };
    if tok.chars().count() == width {
        return tok
            .chars()
            .map(|c| bin(c).ok_or(()))
            .collect::<Result<_, _>>()
            .or_else(|_| {
                err(
                    line,
                    format!("'{tok}' is not a binary value (use 0, 1 and x)"),
                )
            });
    }
    let digits = width.div_ceil(4);
    if tok.chars().count() == digits {
        let mut bits = vec![];
        for (i, c) in tok.chars().enumerate() {
            // the first digit holds only the leftover high bits
            let n = if i == 0 { width - 4 * (digits - 1) } else { 4 };
            if matches!(c, 'x' | 'X' | '-') {
                bits.extend(std::iter::repeat_n(None, n));
                continue;
            }
            let Some(d) = c.to_digit(16) else {
                return err(line, format!("'{tok}' is not a hexadecimal value"));
            };
            if d >> n != 0 {
                return err(line, format!("'{tok}' does not fit in {width} bits"));
            }
            bits.extend((0..n).rev().map(|b| Some(d >> b & 1 == 1)));
        }
        return Ok(bits);
    }
    err(
        line,
        format!("'{tok}' should be {width} binary digits or {digits} hexadecimal digits"),
    )
}

/// Read a table from text. The variables come from the header line.
pub fn import(text: &str) -> Result<AnalyzerModel, TextError> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.split('#').next().unwrap_or("").trim()))
        .filter(|(_, l)| !l.is_empty() && !l.chars().all(|c| c == '~'));
    let Some((hline, header)) = lines.next() else {
        return err(0, "the file has no header line (for example: A B | Y)");
    };
    let mut ins: Vec<Var> = vec![];
    let mut outs: Vec<Var> = vec![];
    let mut seen_bar = false;
    for tok in header.split_whitespace() {
        if tok == "|" {
            if seen_bar {
                return err(hline, "the separator '|' may appear only once");
            }
            seen_bar = true;
            continue;
        }
        let v = Var::parse(tok).map_err(|e| TextError {
            line: hline,
            message: e.to_string(),
        })?;
        if seen_bar { &mut outs } else { &mut ins }.push(v);
    }
    if !seen_bar {
        return err(
            hline,
            "the header needs a '|' between the inputs and the outputs",
        );
    }
    if ins.is_empty() {
        return err(hline, "the truth table has no inputs");
    }
    if outs.is_empty() {
        return err(hline, "the truth table has no outputs");
    }
    let mut model = AnalyzerModel::new();
    for v in &ins {
        model
            .add_input(v.clone())
            .map_err(|e| model_err(hline, e))?;
    }
    for v in &outs {
        model
            .add_output(v.clone())
            .map_err(|e| model_err(hline, e))?;
    }
    let mut table: TruthTable = model.table().clone();
    for (n, line) in lines {
        let toks: Vec<&str> = line.split_whitespace().filter(|t| *t != "|").collect();
        if toks.len() != ins.len() + outs.len() {
            return err(
                n,
                format!(
                    "expected {} values ({} inputs, {} outputs) but found {}",
                    ins.len() + outs.len(),
                    ins.len(),
                    outs.len(),
                    toks.len()
                ),
            );
        }
        let mut pattern: Vec<Option<bool>> = vec![];
        for (v, t) in ins.iter().zip(&toks) {
            pattern.extend(parse_value(t, v.width as usize, n)?);
        }
        let mut entries: Vec<Entry> = vec![];
        for (v, t) in outs.iter().zip(&toks[ins.len()..]) {
            entries.extend(
                parse_value(t, v.width as usize, n)?
                    .into_iter()
                    .map(|b| match b {
                        Some(b) => Entry::from_bool(b),
                        None => Entry::DontCare,
                    }),
            );
        }
        for row in table.rows_matching(&pattern) {
            for (o, e) in entries.iter().enumerate() {
                table.set(row, o, *e);
            }
        }
    }
    model.set_table(table);
    Ok(model)
}

fn model_err(line: usize, e: ModelError) -> TextError {
    TextError {
        line,
        message: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "
# a comment
A B[3..0] | D[3..0]
~~~~~~~~~~~~~~~~~~~
0  0000   |  1010   # trailing comment
0  0001   |  1101
1  ----   |  0000
";

    #[test]
    fn imports_the_documented_format() {
        let m = import(SAMPLE).unwrap();
        assert_eq!(m.input_bits(), ["A", "B[3]", "B[2]", "B[1]", "B[0]"]);
        assert_eq!(m.output_bits(), ["D[3]", "D[2]", "D[1]", "D[0]"]);
        let t = m.table();
        let out = |row: usize| -> String { (0..4).map(|o| t.get(row, o).to_char()).collect() };
        assert_eq!(out(0b00000), "1010");
        assert_eq!(out(0b00001), "1101");
        assert_eq!(out(0b00010), "xxxx"); // not listed
        for row in 0b10000..=0b11111 {
            assert_eq!(out(row), "0000", "row {row}"); // A=1, any B
        }
    }

    #[test]
    fn hexadecimal_values() {
        let m = import("A[7..0] | Y[3..0]\nC3 | 5\n4x | x\n").unwrap();
        let t = m.table();
        let col = |row: usize| -> String { (0..4).map(|o| t.get(row, o).to_char()).collect() };
        assert_eq!(col(0xC3), "0101");
        for row in 0x40..=0x4F {
            assert_eq!(col(row), "xxxx");
        }
        // 'x' in one hex digit covers sixteen rows; the earlier row is untouched
        assert_eq!(col(0xC3), "0101");
    }

    #[test]
    fn short_leading_hex_digit() {
        // 6 bits = 2 hex digits, the first holding two bits
        let m = import("A[5..0] | Y\n3F | 1\n").unwrap();
        assert_eq!(m.table().get(0x3F, 0), Entry::One);
        let e = import("A[5..0] | Y\n7F | 1\n").unwrap_err();
        assert!(e.message.contains("does not fit"), "{e}");
    }

    #[test]
    fn export_then_import_gives_the_same_model() {
        let mut m = AnalyzerModel::new();
        m.add_input(Var::new("A", 2)).unwrap();
        m.add_input(Var::new("en", 1)).unwrap();
        m.add_output(Var::new("Q", 2)).unwrap();
        m.add_output(Var::new("ok", 1)).unwrap();
        let mut t = m.table().clone();
        for row in 0..8 {
            for o in 0..3 {
                t.set(
                    row,
                    o,
                    match (row * 3 + o) % 3 {
                        0 => Entry::Zero,
                        1 => Entry::One,
                        _ => Entry::DontCare,
                    },
                );
            }
        }
        m.set_table(t);
        let text = export(&m);
        let back = import(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(back.inputs(), m.inputs());
        assert_eq!(back.outputs(), m.outputs());
        assert_eq!(back.table(), m.table());
    }

    #[test]
    fn problems_name_their_line() {
        let cases = [
            ("", 0, "no header"),
            ("A B\n", 1, "'|'"),
            ("A | | Y\n", 1, "only once"),
            ("| Y\n", 1, "no inputs"),
            ("A |\n", 1, "no outputs"),
            ("A | Y\n0 1 1\n", 2, "expected 2 values"),
            ("A | Y\n2 1\n", 2, "binary"),
            ("A[4..0] | Y\n000 1\n", 2, "should be"),
            ("A A | Y\n", 1, "already"),
            ("A[bad] | Y\n", 1, "not a valid variable"),
        ];
        for (text, line, what) in cases {
            let e = import(text).unwrap_err();
            assert_eq!(e.line, line, "{text:?}: {e}");
            assert!(e.message.contains(what), "{text:?}: {e}");
        }
    }

    #[test]
    fn imported_model_minimises() {
        let m = import("a b | y\n0 0 | 0\n0 1 | 1\n1 0 | 1\n1 1 | 1\n").unwrap();
        assert_eq!(m.expression(0).to_string(), "a + b");
    }
}
