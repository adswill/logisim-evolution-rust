//! PLA (programmable logic array): maps the input to the output of the first matching table row.
//!
//! Port order: 0 = input at (0,0); 1 = output at (50,0).
//! The program is a text table, one row per line, `inputs outputs # comment`, with the most
//! significant bit leftmost. Inputs use `0`, `1` or `x` (don't care); outputs use `0`/`1`.
//! The flexible Logisim form `0 1 x || 1 0` is accepted as well; saved tables use the compact form.

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    /// Index 0 is the least significant bit.
    inputs: Vec<char>,
    outputs: Vec<char>,
    comment: String,
}

impl Row {
    fn matches(&self, mut input: u64) -> bool {
        for &bit in &self.inputs {
            let b = input & 1;
            if (bit == '1' && b != 1) || (bit == '0' && b != 0) {
                return false;
            }
            input >>= 1;
        }
        true
    }
    fn output(&self) -> u64 {
        self.outputs.iter().enumerate().filter(|(_, c)| **c == '1').fold(0, |acc, (i, _)| acc | 1 << i)
    }
    fn to_line(&self) -> String {
        let i: String = self.inputs.iter().rev().collect();
        let o: String = self.outputs.iter().rev().collect();
        let c = self.comment.trim();
        if c.is_empty() {
            format!("{i} {o}")
        } else {
            format!("{i} {o} # {c}")
        }
    }
}

fn resized(v: &[char], n: usize) -> Vec<char> {
    let mut out = v.to_vec();
    out.resize(n, '0');
    out
}

/// Parse a program. Returns the rows plus the (inputs, outputs) widths they imply, or `None` for no rows.
#[allow(clippy::type_complexity)]
fn parse_table(text: &str) -> Result<Option<(Vec<Row>, usize, usize)>, String> {
    let mut rows: Vec<Row> = Vec::new();
    let mut size: Option<(usize, usize)> = None;
    for line in text.lines() {
        let (body, comment) = match line.split_once('#') {
            Some((b, c)) => (b.trim(), c.trim()),
            None => (line.trim(), ""),
        };
        let (ins, outs): (String, String) = if let Some((i, o)) = body.split_once("||") {
            let strip = |s: &str| s.chars().filter(|c| !c.is_whitespace() && *c != '|').collect::<String>();
            (strip(i), strip(o))
        } else {
            match body.split_once(char::is_whitespace) {
                Some((i, o)) => (i.trim().to_string(), o.trim().to_string()),
                None if body.is_empty() => (String::new(), String::new()),
                None => return Err(format!("row '{line}' needs inputs and outputs separated by a space")),
            }
        };
        if ins.is_empty() && outs.is_empty() {
            continue; // blank or comment-only line
        }
        if ins.is_empty() || outs.is_empty() {
            return Err(format!("row '{line}' needs both inputs and outputs"));
        }
        if let Some(bad) = ins.chars().find(|c| !matches!(c, '0' | '1' | 'x')) {
            return Err(format!("row '{line}': invalid input bit '{bad}' (use 0, 1 or x)"));
        }
        if let Some(bad) = outs.chars().find(|c| !matches!(c, '0' | '1')) {
            return Err(format!("row '{line}': invalid output bit '{bad}' (use 0 or 1)"));
        }
        match size {
            None => size = Some((ins.len(), outs.len())),
            Some((i, _)) if i != ins.len() => return Err(format!("row '{line}' must have exactly {i} input bits")),
            Some((_, o)) if o != outs.len() => return Err(format!("row '{line}' must have exactly {o} output bits")),
            _ => {}
        }
        rows.push(Row { inputs: ins.chars().rev().collect(), outputs: outs.chars().rev().collect(), comment: comment.to_string() });
    }
    Ok(size.map(|(i, o)| (rows, i, o)))
}

#[derive(Debug, Clone)]
pub struct Pla {
    in_width: u8,
    out_width: u8,
    rows: Vec<Row>,
}

impl Pla {
    pub fn new() -> Self {
        Pla { in_width: 2, out_width: 2, rows: Vec::new() }
    }
    /// Output for `input`, or 0 when no row matches (Logisim's behaviour).
    pub fn value_for(&self, input: u64) -> u64 {
        self.rows.iter().find(|r| r.matches(input)).map_or(0, Row::output)
    }
    /// Comment of the row matching `input` (`"n/a"` if none) — shown on the component face.
    pub fn comment_for(&self, input: u64) -> &str {
        self.rows.iter().find(|r| r.matches(input)).map_or("n/a", |r| r.comment.as_str())
    }
    /// The program in the standard text form.
    pub fn table_text(&self) -> String {
        self.rows.iter().map(|r| r.to_line() + "\n").collect()
    }
}

impl Default for Pla {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Pla {
    fn name(&self) -> &str {
        "PLA"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("in_width", self.in_width).with("out_width", self.out_width).with("table", self.table_text())
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("in_width", "Input bits", 1, 32, 2),
            OptionSpec::int("out_width", "Output bits", 1, 32, 2),
            OptionSpec::string("table", "Program (one 'inputs outputs # comment' row per line)", ""),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["in_width", "out_width", "table"])?;
        let mut in_w = m.int("in_width", 1, 32)?.map_or(self.in_width, |v| v as u8);
        let mut out_w = m.int("out_width", 1, 32)?.map_or(self.out_width, |v| v as u8);
        let mut rows = self.rows.clone();
        if let Some(text) = m.string("table")? {
            rows = Vec::new();
            if let Some((parsed, i, o)) = parse_table(text)? {
                // The program decides the widths, as in Logisim; an explicit width must agree.
                for (key, given, actual) in [("in_width", m.int("in_width", 1, 32)?, i), ("out_width", m.int("out_width", 1, 32)?, o)] {
                    if given.is_some_and(|g| g as usize != actual) {
                        return Err(format!("table has {actual} bits but '{key}' is {}", given.unwrap()));
                    }
                }
                if !(1..=32).contains(&i) || !(1..=32).contains(&o) {
                    return Err("table rows must have 1..=32 input and output bits".to_string());
                }
                in_w = i as u8;
                out_w = o as u8;
                rows = parsed;
            }
        }
        for r in &mut rows {
            r.inputs = resized(&r.inputs, in_w as usize);
            r.outputs = resized(&r.outputs, out_w as usize);
        }
        self.in_width = in_w;
        self.out_width = out_w;
        self.rows = rows;
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![
            Port::new(0, 0, PortKind::Input, BitWidth::new(self.in_width)),
            Port::new(50, 0, PortKind::Output, BitWidth::new(self.out_width)),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        // An undefined input reads as all ones, like Logisim's `toLongValue() == -1`.
        let input = ctx.port_value(0).to_u64().unwrap_or(u64::MAX);
        ctx.set_port(1, Value::known(BitWidth::new(self.out_width), self.value_for(input)), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiring::Pin;
    use logisim_core::{Circuit, Direction, Location};

    fn pla(table: &str) -> Pla {
        let mut p = Pla::new();
        p.set_options(&OptionMap::new().with("table", table)).unwrap();
        p
    }

    fn run(p: Pla, input: Value) -> Value {
        let mut c = Circuit::new();
        let id = c.add_component(Location::new(100, 100), Direction::East, Box::new(p));
        let at = c.port_location(id, 0);
        c.add_component(at, Direction::East, Box::new(Pin::new(input)));
        c.reset();
        c.port_value(id, 1)
    }

    #[test]
    fn first_matching_row_wins_and_dont_care() {
        let p = pla("11 10 # both\n1x 01 # high bit\n00 11 # none\n");
        assert_eq!((p.in_width, p.out_width), (2, 2));
        let w = BitWidth::new(2);
        for (inp, want) in [(0b11, 0b10), (0b10, 0b01), (0b00, 0b11), (0b01, 0)] {
            assert_eq!(p.value_for(inp), want, "input {inp:02b}");
            assert_eq!(run(p.clone(), Value::known(w, inp)), Value::known(w, want));
        }
        assert_eq!((p.comment_for(0b11), p.comment_for(0b01)), ("both", "n/a"));
    }

    #[test]
    fn undefined_input_reads_as_all_ones() {
        let p = pla("11 01\n");
        assert_eq!(run(p, Value::unknown(BitWidth::new(2))), Value::known(BitWidth::new(2), 1));
    }

    #[test]
    fn flexible_format_and_widths_follow_the_table() {
        let p = pla("# header\n0 1 x || 1 0 1\n");
        assert_eq!((p.in_width, p.out_width), (3, 3));
        assert_eq!(p.table_text(), "01x 101\n");
        assert_eq!(p.value_for(0b011), 0b101);
        assert_eq!(p.ports()[0].width.get(), 3);
    }

    #[test]
    fn rejects_bad_tables() {
        let mut p = Pla::new();
        for bad in ["1z 01", "11 0x", "11 01\n1 01", "11 01\n11 0", "11"] {
            assert!(p.set_options(&OptionMap::new().with("table", bad)).is_err(), "{bad}");
        }
        assert!(p.set_options(&OptionMap::new().with("table", "11 01").with("in_width", 3)).is_err());
    }

    #[test]
    fn resizing_pads_rows_and_round_trips() {
        let mut p = pla("11 10 # c\n");
        p.set_options(&OptionMap::new().with("in_width", 3).with("out_width", 1)).unwrap();
        assert_eq!(p.table_text(), "011 0 # c\n");
        let mut q = Pla::new();
        q.set_options(&p.options()).unwrap();
        assert_eq!(q.options(), p.options());
    }
}
