//! Test vectors: the file format (port of `data.TestVector`), the evaluator that runs a vector
//! against a circuit (port of `circuit.TestVectorEvaluator`) and a result tracker (the non-GUI
//! part of `gui.test.Model`).
//!
//! # File format
//!
//! ```text
//! # comment
//! a b  q[4] <set> <seq>      <- header: column names, optional [width], optional <set>/<seq>
//! 0 0  0000   0     0         <- rows: one value per column (see `radix::parse_log_string`)
//! 1 1  0xF    0     0         <- `<DC>` = don't care, `<FLOAT>` = expect an undriven (X) output
//! ```
//!
//! Input pins/clocks are driven with the row's values; every other column is compared against
//! the circuit's outputs once it has settled. Rows with a `<set>` > 0 form a sequence: the
//! circuit is reset only at the start of a set, and the clock is toggled between rows.

use std::fmt;
use std::io::{self, Write};

use logisim_core::{BitWidth, Value};

use crate::radix::{binary_display, compatible, parse_log_string, radix_of_log_string};

/// A problem loading or running a test vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestError(pub String);

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TestError {}

fn err<T>(msg: impl Into<String>) -> Result<T, TestError> {
    Err(TestError(msg.into()))
}

/// A parsed test-vector file.
#[derive(Debug, Clone, PartialEq)]
pub struct TestVector {
    pub column_name: Vec<String>,
    pub column_width: Vec<BitWidth>,
    /// Radix (2, 8, 10, 16) each column's first value was written in, for display.
    pub column_radix: Vec<u32>,
    pub data: Vec<Vec<Value>>,
    pub set_numbers: Vec<u32>,
    pub seq_numbers: Vec<u32>,
    /// 1-based source line of each row.
    pub line_numbers: Vec<usize>,
    dont_care: Vec<Vec<bool>>,
    floating: Vec<Vec<bool>>,
}

impl TestVector {
    pub fn from_file(path: &str) -> Result<TestVector, TestError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| TestError(format!("cannot read {path}: {e}")))?;
        TestVector::parse(&text)
    }

    pub fn parse(text: &str) -> Result<TestVector, TestError> {
        // non-empty lines with comments stripped, with their 1-based line numbers
        let mut lines = text.lines().enumerate().filter_map(|(i, l)| {
            let l = l.split('#').next().unwrap_or("");
            let toks: Vec<&str> = l.split_whitespace().collect();
            (!toks.is_empty()).then_some((i + 1, toks))
        });
        let Some((_, header)) = lines.next() else {
            return err("TestVector format error: empty file");
        };
        let mut v = TestVector {
            column_name: vec![],
            column_width: vec![],
            column_radix: vec![],
            data: vec![],
            set_numbers: vec![],
            seq_numbers: vec![],
            line_numbers: vec![],
            dont_care: vec![],
            floating: vec![],
        };
        let (mut set_col, mut seq_col) = (None, None);
        for (i, t) in header.iter().enumerate() {
            match t.to_uppercase().as_str() {
                "<SET>" => set_col = Some(i),
                "<SEQ>" => seq_col = Some(i),
                _ => {
                    let (name, width) = parse_column_spec(t)?;
                    v.column_name.push(name);
                    v.column_width.push(width);
                    v.column_radix.push(2);
                }
            }
        }
        for (line_no, tokens) in lines {
            v.parse_row(line_no, &tokens, set_col, seq_col)?;
        }
        v.check_order()?;
        Ok(v)
    }

    fn parse_row(
        &mut self,
        line: usize,
        tokens: &[&str],
        set_col: Option<usize>,
        seq_col: Option<usize>,
    ) -> Result<(), TestError> {
        let n = self.column_name.len();
        let mut vals = vec![Value::NIL; n];
        let (mut dc, mut float) = (vec![false; n], vec![false; n]);
        let (mut set, mut seq) = (0u32, 0u32);
        let mut pin = 0;
        for (ti, t) in tokens.iter().enumerate() {
            if Some(ti) == set_col {
                set = t.parse().map_err(|_| {
                    TestError(format!(
                        "Test Vector data format error: invalid set value: {t}"
                    ))
                })?;
                continue;
            }
            if Some(ti) == seq_col {
                seq = t.parse().map_err(|_| {
                    TestError(format!(
                        "Test Vector data format error: invalid seq value: {t}"
                    ))
                })?;
                continue;
            }
            if pin >= n {
                return err("Test Vector data format error: too many values");
            }
            let width = self.column_width[pin];
            match t.to_uppercase().as_str() {
                "<DC>" => {
                    dc[pin] = true;
                    vals[pin] = Value::UNKNOWN;
                }
                "<FLOAT>" => {
                    float[pin] = true;
                    vals[pin] = Value::unknown(width);
                }
                _ => {
                    vals[pin] = parse_log_string(width, t).map_err(|m| {
                        TestError(format!(
                            "Test Vector data format error: {}",
                            suggest_column(&m, &self.column_name[pin])
                        ))
                    })?;
                    if self.data.is_empty() {
                        self.column_radix[pin] = radix_of_log_string(width, t);
                    }
                }
            }
            pin += 1;
        }
        if pin < n {
            return err("Test Vector data format error: not enough values");
        }
        self.data.push(vals);
        self.dont_care.push(dc);
        self.floating.push(float);
        self.set_numbers.push(set);
        self.seq_numbers.push(seq);
        self.line_numbers.push(line);
        Ok(())
    }

    fn check_order(&self) -> Result<(), TestError> {
        let (mut last_set, mut last_seq) = (0, 0);
        for (&set, &seq) in self.set_numbers.iter().zip(&self.seq_numbers) {
            if set < last_set {
                return err(format!(
                    "<Set> numbers out of order: {last_set} before {set}"
                ));
            }
            if set == last_set && set > 0 && seq <= last_seq {
                return err(format!(
                    "<Seq> numbers out of order: {last_seq} before {seq} in set {set}"
                ));
            }
            if set == 0 && seq != 0 {
                return err(format!("<Set> is 0 but <Seq> is {seq}, not 0"));
            }
            if set != 0 && seq == 0 {
                return err(format!("<Set> is {set} which not 0 but <Seq> is 0"));
            }
            (last_set, last_seq) = (set, seq);
        }
        Ok(())
    }

    pub fn row_count(&self) -> usize {
        self.data.len()
    }
    pub fn is_dont_care(&self, row: usize, col: usize) -> bool {
        self.dont_care
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .unwrap_or(false)
    }
    pub fn is_floating(&self, row: usize, col: usize) -> bool {
        self.floating
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .unwrap_or(false)
    }
    /// `"<FLOAT>"` / `"<DC>"` if any row of the column uses it (floating wins), else `None`.
    pub fn special_column_entry(&self, col: usize) -> Option<&'static str> {
        if (0..self.floating.len()).any(|r| self.is_floating(r, col)) {
            Some("<FLOAT>")
        } else if (0..self.dont_care.len()).any(|r| self.is_dont_care(r, col)) {
            Some("<DC>")
        } else {
            None
        }
    }
}

fn parse_column_spec(t: &str) -> Result<(String, BitWidth), TestError> {
    let Some(s) = t.find('[') else {
        return Ok((t.to_string(), BitWidth::ONE));
    };
    let e = t.find(']');
    if e != Some(t.len() - 1) || s == 0 || e == Some(s + 1) {
        return err(format!("Test Vector header format error: bad spec: {t}"));
    }
    match t[s + 1..t.len() - 1].parse::<u8>() {
        Ok(w) if (1..=64).contains(&w) => Ok((t[..s].to_string(), BitWidth::new(w))),
        _ => err(format!("Test Vector header format error: bad width: {t}")),
    }
}

/// Turns `did you mean [8]?` into `did you mean NAME[8]?` so the hint names the column.
fn suggest_column(msg: &str, column: &str) -> String {
    match (msg.contains("Too many bits"), msg.find("did you mean [")) {
        (true, Some(start)) => {
            let after = start + "did you mean [".len();
            match msg[after..].find(']') {
                Some(end) => format!(
                    "{}did you mean {column}[{}]?{}",
                    &msg[..start],
                    &msg[after..after + end],
                    msg[after + end + 1..].trim_start_matches('?')
                ),
                None => msg.to_string(),
            }
        }
        _ => msg.to_string(),
    }
}

/// A mismatch between a column's expected and computed value in one row.
#[derive(Debug, Clone, PartialEq)]
pub struct LineReport {
    pub column: usize,
    pub column_name: String,
    pub expected: Value,
    pub computed: Value,
    pub oscillating: bool,
}

impl fmt::Display for LineReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} = {} (expected {}){}",
            self.column_name,
            binary_display(&self.computed),
            binary_display(&self.expected),
            if self.oscillating { " oscillating" } else { "" }
        )
    }
}

/// What the evaluator needs from a simulated circuit. Columns are the vector's columns.
pub trait Rig {
    /// Reset the simulation and let it settle.
    fn reset(&mut self);
    /// Advance clocks by one half period and let that reach the wires.
    fn toggle_clocks(&mut self);
    /// Apply driven inputs and let the circuit settle.
    fn propagate(&mut self);
    fn is_oscillating(&self) -> bool;
    /// True if the column is an input pin that the vector drives.
    fn is_input(&self, col: usize) -> bool;
    fn is_clock(&self, col: usize) -> bool;
    /// Current value of the column's pin (or clock).
    fn read(&mut self, col: usize) -> Value;
    /// Set an input pin; takes effect at the next [`propagate`](Rig::propagate).
    fn drive(&mut self, col: usize, v: Value);
}

/// Runs a [`TestVector`] against a [`Rig`].
#[derive(Debug, Clone)]
pub struct TestVectorEvaluator<'a> {
    pub vector: &'a TestVector,
    /// Rows to run, in order (default: all rows in file order).
    pub steps: Vec<usize>,
    /// Reset at the start of each set (default true).
    pub allow_reset: bool,
    /// Settle after the last row too (default true).
    pub propagate_on_last: bool,
}

impl<'a> TestVectorEvaluator<'a> {
    pub fn new(vector: &'a TestVector) -> Self {
        TestVectorEvaluator {
            vector,
            steps: (0..vector.row_count()).collect(),
            allow_reset: true,
            propagate_on_last: true,
        }
    }

    /// Run the steps; `on_row(row, report)` is called per row with the mismatches (empty = pass),
    /// and returns false to cancel. Returns `(passed, failed)`.
    pub fn evaluate(
        &self,
        rig: &mut dyn Rig,
        mut on_row: impl FnMut(usize, &[LineReport]) -> bool,
    ) -> (usize, usize) {
        let v = self.vector;
        let (mut pass, mut fail) = (0, 0);
        let mut current_set = u32::MAX;
        let mut current_seq = 0;
        for &row in &self.steps {
            if row >= v.row_count() {
                continue;
            }
            let (set, seq) = (v.set_numbers[row], v.seq_numbers[row]);
            let should_reset = seq == 0 || current_seq == 0 || set != current_set;
            (current_set, current_seq) = (set, seq);
            if self.allow_reset && should_reset {
                rig.reset();
            }
            if !should_reset && !rig.is_oscillating() {
                rig.toggle_clocks();
            }
            if !rig.is_oscillating() {
                for col in 0..v.column_name.len() {
                    if rig.is_input(col) && !rig.is_clock(col) {
                        let want = v.data[row][col];
                        if rig.read(col) != want {
                            rig.drive(col, want);
                        }
                    }
                }
                if Some(&row) != self.steps.last() || self.propagate_on_last {
                    rig.propagate();
                }
            }
            let mut report = vec![];
            for col in 0..v.column_name.len() {
                if rig.is_input(col) && !rig.is_clock(col) {
                    continue;
                }
                if v.is_dont_care(row, col) {
                    continue;
                }
                let expected = v.data[row][col];
                let line = |computed, oscillating| LineReport {
                    column: col,
                    column_name: v.column_name[col].clone(),
                    expected,
                    computed,
                    oscillating,
                };
                if rig.is_oscillating() {
                    report.push(line(Value::ERROR, true));
                    continue;
                }
                let got = rig.read(col);
                if v.is_floating(row, col) {
                    if !got.is_unknown() {
                        report.push(line(got, false));
                    }
                } else if !compatible(&expected, &got) {
                    report.push(line(got, false));
                }
            }
            if report.is_empty() {
                pass += 1;
            } else {
                fail += 1;
            }
            if !on_row(row, &report) {
                break;
            }
        }
        (pass, fail)
    }

    /// Run everything and print a command-line report like Logisim's `-test-vector` mode;
    /// returns the number of failed rows.
    pub fn run_cli(&self, rig: &mut dyn Rig, out: &mut impl Write) -> io::Result<usize> {
        writeln!(out, "Running {} vectors…", self.vector.row_count())?;
        let mut io_err = None;
        let (pass, fail) = self.evaluate(rig, |row, report| {
            if !report.is_empty() {
                if let Err(e) = write_failure(out, self.vector, row, report) {
                    io_err = Some(e);
                    return false;
                }
            }
            true
        });
        if let Some(e) = io_err {
            return Err(e);
        }
        writeln!(out, "Passed: {pass}, Failed: {fail}")?;
        Ok(fail)
    }
}

/// "Error on test vector 3 (set 1, sequence 2, line 9):" followed by one indented line per mismatch.
pub fn failure_text(v: &TestVector, row: usize, report: &[LineReport]) -> String {
    let number = row + 1;
    let line = v.line_numbers[row];
    let location = if v.set_numbers[row] > 0 {
        format!(
            "{number} (set {}, sequence {}, line {line})",
            v.set_numbers[row], v.seq_numbers[row]
        )
    } else {
        format!("{number} (line {line})")
    };
    let mut s = format!("Error on test vector {location}:\n");
    for r in report {
        s.push_str(&format!("  {r}\n"));
    }
    s
}

fn write_failure(
    out: &mut impl Write,
    v: &TestVector,
    row: usize,
    report: &[LineReport],
) -> io::Result<()> {
    out.write_all(failure_text(v, row, report).as_bytes())
}

/// Pass/fail bookkeeping for a vector being run (the non-GUI part of `gui.test.Model`).
#[derive(Debug, Clone)]
pub struct TestRun {
    vector: TestVector,
    results: Vec<Option<Vec<LineReport>>>,
    num_pass: usize,
    num_fail: usize,
    sorted: Vec<usize>,
}

impl TestRun {
    pub fn new(vector: TestVector) -> Self {
        let n = vector.row_count();
        let mut t = TestRun {
            vector,
            results: vec![None; n],
            num_pass: 0,
            num_fail: 0,
            sorted: vec![],
        };
        t.resort();
        t
    }
    pub fn vector(&self) -> &TestVector {
        &self.vector
    }
    pub fn pass(&self) -> usize {
        self.num_pass
    }
    pub fn fail(&self) -> usize {
        self.num_fail
    }
    /// Rows run so far (`pass + fail`).
    pub fn done(&self) -> usize {
        self.num_pass + self.num_fail
    }
    /// Mismatches of a finished row (`None` = passed or not run yet).
    pub fn result(&self, row: usize) -> Option<&[LineReport]> {
        self.results.get(row).and_then(|r| r.as_deref())
    }
    pub fn clear(&mut self) {
        self.results.iter_mut().for_each(|r| *r = None);
        self.num_pass = 0;
        self.num_fail = 0;
        self.resort();
    }

    /// Record the next row's result. Rows must arrive in order; returns false (and records
    /// nothing) for any other row, which tells a runner it was superseded.
    pub fn set_result(&mut self, row: usize, report: &[LineReport]) -> bool {
        if row >= self.results.len() || row != self.done() {
            return false;
        }
        if report.is_empty() {
            self.num_pass += 1;
        } else {
            self.results[row] = Some(report.to_vec());
            self.num_fail += 1;
        }
        self.resort();
        true
    }

    /// Replace any row's result (e.g. after re-running one row), keeping the counts consistent.
    pub fn update_result(&mut self, row: usize, report: Option<Vec<LineReport>>) {
        if row >= self.results.len() {
            return;
        }
        let was_counted = row < self.done();
        let old_failed = self.results[row].is_some();
        let new_failed = report.as_ref().is_some_and(|r| !r.is_empty());
        self.results[row] = report.filter(|r| !r.is_empty());
        if was_counted {
            if !old_failed && new_failed {
                self.num_pass -= 1;
                self.num_fail += 1;
            } else if old_failed && !new_failed {
                self.num_pass += 1;
                self.num_fail -= 1;
            }
        } else if new_failed {
            self.num_fail += 1;
        } else {
            self.num_pass += 1;
        }
        self.resort();
    }

    /// Run every row with `rig`, recording results (returns `(passed, failed)`).
    pub fn run(&mut self, rig: &mut dyn Rig) -> (usize, usize) {
        self.clear();
        let vector = self.vector.clone();
        let eval = TestVectorEvaluator::new(&vector);
        eval.evaluate(rig, |row, report| self.set_result(row, report))
    }

    /// Row shown at display position `i`: ordered by set, then sequence, with failures before
    /// passes within a tie, then file order.
    pub fn sorted_index(&self, i: usize) -> usize {
        self.sorted.get(i).copied().unwrap_or(i)
    }

    fn resort(&mut self) {
        let v = &self.vector;
        let mut idx: Vec<usize> = (0..v.row_count()).collect();
        idx.sort_by(|&a, &b| {
            (v.set_numbers[a], v.seq_numbers[a])
                .cmp(&(v.set_numbers[b], v.seq_numbers[b]))
                .then_with(|| self.results[b].is_some().cmp(&self.results[a].is_some()))
                .then(a.cmp(&b))
        });
        self.sorted = idx;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(n: u8) -> BitWidth {
        BitWidth::new(n)
    }

    #[test]
    fn parses_header_rows_and_comments() {
        let v = TestVector::parse(
            "# adder\n a b[4]  sum[5] <set> <seq>   # header\n0 0000 00000 0 0\n1 0xF 0b10000 1 1\n\n0 <DC> <FLOAT> 1 2\n",
        )
        .unwrap();
        assert_eq!(v.column_name, ["a", "b", "sum"]);
        assert_eq!(v.column_width, [w(1), w(4), w(5)]);
        assert_eq!(v.row_count(), 3);
        assert_eq!(v.data[1][1], Value::known(w(4), 15));
        assert_eq!(v.data[1][2], Value::known(w(5), 16));
        assert_eq!(v.column_radix, [2, 2, 2]);
        assert_eq!(v.set_numbers, [0, 1, 1]);
        assert_eq!(v.seq_numbers, [0, 1, 2]);
        assert_eq!(v.line_numbers, [3, 4, 6]);
        assert!(v.is_dont_care(2, 1) && v.is_floating(2, 2) && !v.is_floating(2, 1));
        assert_eq!(v.special_column_entry(2), Some("<FLOAT>"));
        assert_eq!(v.special_column_entry(1), Some("<DC>"));
        assert_eq!(v.special_column_entry(0), None);
        assert_eq!(v.data[2][2], Value::unknown(w(5)));
    }

    #[test]
    fn first_row_decides_display_radix() {
        let v = TestVector::parse("a[8] b[8]\n0xFF 7\n").unwrap();
        assert_eq!(v.column_radix, [16, 10]);
    }

    #[test]
    fn format_errors_are_specific() {
        let e = |t: &str| TestVector::parse(t).unwrap_err().0;
        assert_eq!(e("# nothing\n\n"), "TestVector format error: empty file");
        assert!(e("a[0]\n").contains("bad width: a[0]"));
        assert!(e("a[x]\n").contains("bad width"));
        assert!(e("[4]\n").contains("bad spec"));
        assert!(e("a b\n1\n").contains("not enough values"));
        assert!(e("a\n1 0\n").contains("too many values"));
        assert!(e("a <set> <seq>\n0 x 0\n").contains("invalid set value: x"));
        let m = e("a[4]\n0xFF\n");
        assert!(m.contains("did you mean a[8]?"), "{m}");
        assert!(e("a <set> <seq>\n0 1 1\n0 0 1\n").contains("out of order"));
        assert!(e("a <set> <seq>\n0 1 0\n").contains("which not 0 but <Seq> is 0"));
        assert!(e("a <set> <seq>\n0 0 3\n").contains("<Set> is 0 but <Seq> is 3"));
        assert!(e("a <set> <seq>\n0 1 1\n0 1 1\n")
            .contains("<Seq> numbers out of order: 1 before 1 in set 1"));
    }

    /// A scripted fake circuit: output `q` = a AND b, plus a clock-counted output.
    struct Fake {
        a: Value,
        b: Value,
        pending: Vec<(usize, Value)>,
        q: Value,
        count: u64,
        resets: usize,
        osc: bool,
    }
    impl Fake {
        fn new() -> Self {
            Fake {
                a: Value::FALSE,
                b: Value::FALSE,
                pending: vec![],
                q: Value::FALSE,
                count: 0,
                resets: 0,
                osc: false,
            }
        }
    }
    // columns: 0 = a (input), 1 = b (input), 2 = q (output), 3 = clk (clock)
    impl Rig for Fake {
        fn reset(&mut self) {
            self.resets += 1;
            self.count = 0;
        }
        fn toggle_clocks(&mut self) {
            self.count += 1;
        }
        fn propagate(&mut self) {
            for (c, v) in self.pending.drain(..) {
                if c == 0 {
                    self.a = v;
                } else {
                    self.b = v;
                }
            }
            self.q = self.a.and(self.b);
        }
        fn is_oscillating(&self) -> bool {
            self.osc
        }
        fn is_input(&self, c: usize) -> bool {
            c == 0 || c == 1 || c == 3
        }
        fn is_clock(&self, c: usize) -> bool {
            c == 3
        }
        fn read(&mut self, c: usize) -> Value {
            match c {
                0 => self.a,
                1 => self.b,
                2 => self.q,
                _ => {
                    if self.count % 2 == 1 {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
            }
        }
        fn drive(&mut self, c: usize, v: Value) {
            self.pending.push((c, v));
        }
    }

    #[test]
    fn evaluator_reports_mismatches() {
        let v = TestVector::parse("a b q\n0 0 0\n1 1 1\n1 0 1\n0 1 <DC>\n").unwrap();
        let mut rig = Fake::new();
        let mut reports = vec![];
        let (p, f) = TestVectorEvaluator::new(&v).evaluate(&mut rig, |row, r| {
            reports.push((row, r.to_vec()));
            true
        });
        assert_eq!((p, f), (3, 1));
        assert_eq!(reports[2].1.len(), 1);
        assert_eq!(reports[2].1[0].to_string(), "q = 0 (expected 1)");
        assert_eq!(rig.resets, 4); // every row of set 0 resets
    }

    #[test]
    fn float_columns_and_oscillation() {
        let v = TestVector::parse("a b q\n1 1 <FLOAT>\n").unwrap();
        let mut rig = Fake::new();
        let (p, f) = TestVectorEvaluator::new(&v).evaluate(&mut rig, |_, _| true);
        assert_eq!((p, f), (0, 1)); // q is driven (1), not floating
        let v = TestVector::parse("a b q\n1 1 1\n").unwrap();
        let mut rig = Fake::new();
        rig.osc = true;
        let mut got = vec![];
        let (p, f) = TestVectorEvaluator::new(&v).evaluate(&mut rig, |_, r| {
            got = r.to_vec();
            true
        });
        assert_eq!((p, f), (0, 1));
        assert!(got[0].oscillating);
        assert_eq!(got[0].to_string(), "q = E (expected 1) oscillating");
    }

    #[test]
    fn sequences_reset_once_and_toggle_the_clock() {
        let text = "a b q clk <set> <seq>\n0 0 0 0 0 0\n1 1 1 0 1 1\n1 1 1 1 1 2\n1 1 1 0 1 3\n";
        let v = TestVector::parse(text).unwrap();
        let mut rig = Fake::new();
        let (p, f) = TestVectorEvaluator::new(&v).evaluate(&mut rig, |_, _| true);
        assert_eq!((p, f), (4, 0));
        assert_eq!(rig.resets, 2); // once for the set, once for the plain row
    }

    #[test]
    fn cancel_stops_early_and_run_cli_output() {
        let v = TestVector::parse("a b q\n0 0 0\n1 1 0\n0 0 0\n").unwrap();
        let mut rig = Fake::new();
        let mut rows = 0;
        TestVectorEvaluator::new(&v).evaluate(&mut rig, |_, _| {
            rows += 1;
            rows < 2
        });
        assert_eq!(rows, 2);
        let mut out = Vec::new();
        let failures = TestVectorEvaluator::new(&v)
            .run_cli(&mut Fake::new(), &mut out)
            .unwrap();
        assert_eq!(failures, 1);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "Running 3 vectors…\nError on test vector 2 (line 3):\n  q = 1 (expected 0)\nPassed: 2, Failed: 1\n"
        );
    }

    #[test]
    fn run_tracks_counts_and_sorts_failures_first() {
        let v = TestVector::parse("a b q\n0 0 0\n1 1 0\n1 1 1\n").unwrap();
        let mut run = TestRun::new(v);
        assert_eq!((run.pass(), run.fail()), (0, 0));
        assert_eq!(run.run(&mut Fake::new()), (2, 1));
        assert_eq!((run.pass(), run.fail(), run.done()), (2, 1, 3));
        assert!(run.result(1).is_some() && run.result(0).is_none());
        // all rows are in set 0/seq 0, so ordering falls back to failed-first
        assert_eq!([0, 1, 2].map(|i| run.sorted_index(i)), [1, 0, 2]);
        // out-of-order results are refused
        run.clear();
        assert!(!run.set_result(1, &[]));
        assert!(run.set_result(0, &[]));
        // fixing a failed row moves it to the pass count
        run.run(&mut Fake::new());
        run.update_result(1, None);
        assert_eq!((run.pass(), run.fail()), (3, 0));
    }

    #[test]
    fn failure_text_mentions_set_and_sequence() {
        let v = TestVector::parse("a q <set> <seq>\n1 0 2 1\n").unwrap();
        let r = LineReport {
            column: 1,
            column_name: "q".into(),
            expected: Value::FALSE,
            computed: Value::TRUE,
            oscillating: false,
        };
        assert_eq!(
            failure_text(&v, 0, &[r]),
            "Error on test vector 1 (set 2, sequence 1, line 2):\n  q = 1 (expected 0)\n"
        );
    }
}
