//! Truth tables: one row for every combination of input bits, one column per output bit.

use crate::expr::Expr;
use std::fmt;

/// One cell of an output column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Entry {
    Zero,
    One,
    /// Either value is acceptable; the minimiser may use it.
    DontCare,
    /// The circuit produced an error or floating value for this row.
    BusError,
}

impl Entry {
    pub fn to_char(self) -> char {
        match self {
            Entry::Zero => '0',
            Entry::One => '1',
            Entry::DontCare => 'x',
            Entry::BusError => 'E',
        }
    }

    /// `0`, `1`, `x`/`X`/`-` (don't care).
    pub fn from_char(c: char) -> Option<Entry> {
        match c {
            '0' => Some(Entry::Zero),
            '1' => Some(Entry::One),
            'x' | 'X' | '-' => Some(Entry::DontCare),
            _ => None,
        }
    }

    /// The next value when a person clicks a cell: 0, 1, x, then 0 again.
    pub fn cycle(self) -> Entry {
        match self {
            Entry::Zero => Entry::One,
            Entry::One => Entry::DontCare,
            Entry::DontCare | Entry::BusError => Entry::Zero,
        }
    }

    pub fn from_bool(b: bool) -> Entry {
        if b {
            Entry::One
        } else {
            Entry::Zero
        }
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_char())
    }
}

/// Input bit `col` (0 = the leftmost, most significant column) is bit `n-1-col` of the row number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TruthTable {
    inputs: usize,
    columns: Vec<Vec<Entry>>,
}

impl TruthTable {
    /// Largest supported number of input bits (a million rows).
    pub const MAX_INPUTS: usize = 20;

    /// A table whose outputs are all don't care.
    pub fn new(inputs: usize, outputs: usize) -> Self {
        assert!(inputs <= Self::MAX_INPUTS, "too many inputs");
        TruthTable {
            inputs,
            columns: vec![vec![Entry::DontCare; 1 << inputs]; outputs],
        }
    }

    pub fn input_count(&self) -> usize {
        self.inputs
    }
    pub fn output_count(&self) -> usize {
        self.columns.len()
    }
    pub fn row_count(&self) -> usize {
        1 << self.inputs
    }

    /// Value of input column `col` in `row`.
    pub fn input_bit(&self, row: usize, col: usize) -> bool {
        assert!(col < self.inputs);
        row >> (self.inputs - 1 - col) & 1 == 1
    }

    /// Row number for the given input bits (leftmost column first).
    pub fn row_of(bits: &[bool]) -> usize {
        bits.iter().fold(0, |r, b| r << 1 | *b as usize)
    }

    pub fn get(&self, row: usize, out: usize) -> Entry {
        self.columns[out][row]
    }
    pub fn set(&mut self, row: usize, out: usize, e: Entry) {
        self.columns[out][row] = e;
    }
    pub fn column(&self, out: usize) -> &[Entry] {
        &self.columns[out]
    }
    pub fn set_column(&mut self, out: usize, entries: Vec<Entry>) {
        assert_eq!(entries.len(), self.row_count());
        self.columns[out] = entries;
    }
    pub fn fill_column(&mut self, out: usize, e: Entry) {
        self.columns[out].fill(e);
    }

    /// Rows matching a pattern with `None` meaning "either value" (leftmost input first).
    pub fn rows_matching(&self, pattern: &[Option<bool>]) -> Vec<usize> {
        assert_eq!(pattern.len(), self.inputs);
        let (mut value, mut free) = (0usize, 0usize);
        for (col, p) in pattern.iter().enumerate() {
            let bit = 1 << (self.inputs - 1 - col);
            match p {
                Some(true) => value |= bit,
                Some(false) => {}
                None => free |= bit,
            }
        }
        (0..self.row_count())
            .filter(|r| r & !free == value)
            .collect()
    }

    /// Fill an output column by evaluating `expr`; `names` gives each input column's variable.
    pub fn set_column_from_expr(&mut self, out: usize, expr: &Expr, names: &[String]) {
        assert_eq!(names.len(), self.inputs);
        for row in 0..self.row_count() {
            let v = expr.eval(&|n| {
                names
                    .iter()
                    .position(|x| x == n)
                    .is_some_and(|col| self.input_bit(row, col))
            });
            self.columns[out][row] = Entry::from_bool(v);
        }
    }

    /// Add an output column at `at` (all don't care).
    pub fn insert_output(&mut self, at: usize) {
        self.columns
            .insert(at, vec![Entry::DontCare; self.row_count()]);
    }
    pub fn remove_output(&mut self, at: usize) -> Vec<Entry> {
        self.columns.remove(at)
    }
    pub fn move_output(&mut self, from: usize, to: usize) {
        let c = self.columns.remove(from);
        self.columns.insert(to, c);
    }

    /// Change the input columns. New column `j` is old column `map[j]`, or a brand new column when
    /// `None`. Old columns left out of `map` are dropped, keeping the rows where they were 0.
    /// A new column copies the old entries into both of its halves.
    pub fn remap_inputs(&mut self, map: &[Option<usize>]) {
        assert!(map.len() <= Self::MAX_INPUTS);
        let n_new = map.len();
        let rows = 1usize << n_new;
        let old_n = self.inputs;
        let mut new_cols = Vec::with_capacity(self.columns.len());
        for col in &self.columns {
            let mut out = Vec::with_capacity(rows);
            for r in 0..rows {
                let mut old_row = 0usize;
                for (j, m) in map.iter().enumerate() {
                    if let Some(o) = m {
                        if r >> (n_new - 1 - j) & 1 == 1 {
                            old_row |= 1 << (old_n - 1 - o);
                        }
                    }
                }
                out.push(col[old_row]);
            }
            new_cols.push(out);
        }
        self.columns = new_cols;
        self.inputs = n_new;
    }

    /// Copy of the table with only the given output columns, in that order.
    pub fn select_outputs(&self, outs: &[usize]) -> TruthTable {
        TruthTable {
            inputs: self.inputs,
            columns: outs.iter().map(|o| self.columns[*o].clone()).collect(),
        }
    }

    /// Row numbers that hold `e` in output `out`.
    pub fn rows_with(&self, out: usize, e: Entry) -> Vec<usize> {
        self.columns[out]
            .iter()
            .enumerate()
            .filter(|(_, x)| **x == e)
            .map(|(i, _)| i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn new_table_is_all_dont_care() {
        let t = TruthTable::new(3, 2);
        assert_eq!(
            (t.input_count(), t.output_count(), t.row_count()),
            (3, 2, 8)
        );
        assert!((0..8).all(|r| t.get(r, 1) == Entry::DontCare));
    }

    #[test]
    fn first_column_is_the_most_significant_bit() {
        let t = TruthTable::new(3, 1);
        assert!(t.input_bit(0b100, 0) && !t.input_bit(0b100, 1) && !t.input_bit(0b100, 2));
        assert!(t.input_bit(0b001, 2));
        assert_eq!(TruthTable::row_of(&[true, false, true]), 0b101);
    }

    #[test]
    fn entry_cycles_and_parses() {
        assert_eq!(Entry::Zero.cycle(), Entry::One);
        assert_eq!(Entry::One.cycle(), Entry::DontCare);
        assert_eq!(Entry::DontCare.cycle(), Entry::Zero);
        assert_eq!(Entry::from_char('-'), Some(Entry::DontCare));
        assert_eq!(Entry::from_char('X'), Some(Entry::DontCare));
        assert_eq!(Entry::from_char('2'), None);
        assert_eq!(Entry::One.to_string(), "1");
    }

    #[test]
    fn rows_matching_patterns() {
        let t = TruthTable::new(3, 1);
        assert_eq!(
            t.rows_matching(&[Some(true), None, Some(false)]),
            [0b100, 0b110]
        );
        assert_eq!(t.rows_matching(&[None, None, None]).len(), 8);
        assert_eq!(
            t.rows_matching(&[Some(false), Some(true), Some(true)]),
            [0b011]
        );
    }

    #[test]
    fn column_from_expression() {
        let mut t = TruthTable::new(2, 1);
        let e = Expr::xor(Expr::var("a"), Expr::var("b"));
        t.set_column_from_expr(0, &e, &names(&["a", "b"]));
        let got: Vec<_> = t.column(0).iter().map(|e| e.to_char()).collect();
        assert_eq!(got, ['0', '1', '1', '0']);
        // the same expression with the columns swapped gives the same function
        t.set_column_from_expr(0, &Expr::var("a"), &names(&["b", "a"]));
        let got: Vec<_> = t.column(0).iter().map(|e| e.to_char()).collect();
        assert_eq!(got, ['0', '1', '0', '1']);
    }

    #[test]
    fn remap_adds_dropped_and_swapped_columns() {
        // y = a & !b  over columns (a, b)
        let mut t = TruthTable::new(2, 1);
        t.set_column_from_expr(
            0,
            &Expr::and(Expr::var("a"), Expr::not(Expr::var("b"))),
            &names(&["a", "b"]),
        );
        let ones = |t: &TruthTable| t.rows_with(0, Entry::One);
        assert_eq!(ones(&t), [0b10]);

        let mut swapped = t.clone();
        swapped.remap_inputs(&[Some(1), Some(0)]);
        assert_eq!(ones(&swapped), [0b01]);

        let mut added = t.clone();
        added.remap_inputs(&[Some(0), Some(1), None]);
        assert_eq!(ones(&added), [0b100, 0b101]);

        let mut front = t.clone();
        front.remap_inputs(&[None, Some(0), Some(1)]);
        assert_eq!(ones(&front), [0b010, 0b110]);

        let mut dropped = t.clone();
        dropped.remap_inputs(&[Some(0)]); // b dropped: keeps the rows where b = 0
        assert_eq!(ones(&dropped), [1]);
        let mut dropped_a = t.clone();
        dropped_a.remap_inputs(&[Some(1)]);
        assert_eq!(dropped_a.rows_with(0, Entry::Zero), [0, 1]);
    }

    #[test]
    fn output_columns_can_be_added_removed_and_moved() {
        let mut t = TruthTable::new(1, 2);
        t.set(0, 0, Entry::One);
        t.set(1, 1, Entry::Zero);
        t.insert_output(1);
        assert_eq!(t.output_count(), 3);
        assert_eq!(t.get(0, 1), Entry::DontCare);
        t.move_output(0, 2);
        assert_eq!(t.get(0, 2), Entry::One);
        assert_eq!(t.remove_output(2)[0], Entry::One);
        let s = t.select_outputs(&[1]);
        assert_eq!(s.output_count(), 1);
    }
}
