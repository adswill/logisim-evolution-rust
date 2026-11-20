//! The analysis model: input and output variables, the truth table, and each output's expression.

use crate::expr::Expr;
use crate::implicant::{self, Form, Minimal};
use crate::parser::{self, ParseError};
use crate::table::{Entry, TruthTable};
use crate::var::{Var, VarError, VarList};
use std::fmt;

pub const MAX_INPUT_BITS: usize = TruthTable::MAX_INPUTS;
pub const MAX_OUTPUT_BITS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    Var(VarError),
    Parse(ParseError),
    /// The expression uses something that is not an input bit.
    UnknownVariable(String),
    NoSuchOutput(String),
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::Var(e) => e.fmt(f),
            ModelError::Parse(e) => e.fmt(f),
            ModelError::UnknownVariable(n) => write!(f, "'{n}' is not an input variable"),
            ModelError::NoSuchOutput(n) => write!(f, "there is no output named '{n}'"),
        }
    }
}

impl std::error::Error for ModelError {}

impl From<VarError> for ModelError {
    fn from(e: VarError) -> Self {
        ModelError::Var(e)
    }
}
impl From<ParseError> for ModelError {
    fn from(e: ParseError) -> Self {
        ModelError::Parse(e)
    }
}

/// Inputs, outputs, truth table and expressions of one analysis session.
///
/// Columns of the table are bits: a variable `D[3..0]` contributes four columns
/// `D[3] D[2] D[1] D[0]`. Expressions work on those bit names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzerModel {
    inputs: VarList,
    outputs: VarList,
    table: TruthTable,
    /// Expression typed or chosen for each output bit; `None` means "derive it from the table".
    exprs: Vec<Option<Expr>>,
}

impl Default for AnalyzerModel {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyzerModel {
    pub fn new() -> Self {
        AnalyzerModel {
            inputs: VarList::new(MAX_INPUT_BITS),
            outputs: VarList::new(MAX_OUTPUT_BITS),
            table: TruthTable::new(0, 0),
            exprs: vec![],
        }
    }

    /// A model from variables and a table whose shape matches them.
    pub fn from_parts(
        inputs: &[Var],
        outputs: &[Var],
        table: TruthTable,
    ) -> Result<Self, ModelError> {
        let mut m = AnalyzerModel::new();
        for v in inputs {
            m.inputs.add(v.clone(), &[&m.outputs.clone()])?;
        }
        for v in outputs {
            m.outputs.add(v.clone(), &[&m.inputs.clone()])?;
        }
        assert_eq!(
            table.input_count(),
            m.inputs.bit_count(),
            "table inputs do not match"
        );
        assert_eq!(
            table.output_count(),
            m.outputs.bit_count(),
            "table outputs do not match"
        );
        m.exprs = vec![None; table.output_count()];
        m.table = table;
        Ok(m)
    }

    pub fn inputs(&self) -> &VarList {
        &self.inputs
    }
    pub fn outputs(&self) -> &VarList {
        &self.outputs
    }
    pub fn table(&self) -> &TruthTable {
        &self.table
    }
    /// Input bit names in column order.
    pub fn input_bits(&self) -> Vec<String> {
        self.inputs.bits()
    }
    pub fn output_bits(&self) -> Vec<String> {
        self.outputs.bits()
    }

    fn output_index(&self, bit: &str) -> Result<usize, ModelError> {
        self.output_bits()
            .iter()
            .position(|b| b == bit)
            .ok_or_else(|| ModelError::NoSuchOutput(bit.to_string()))
    }

    // ---------- variables ----------

    pub fn add_input(&mut self, v: Var) -> Result<(), ModelError> {
        let others = self.outputs.clone();
        let old_bits = self.inputs.bit_count();
        self.inputs.add(v.clone(), &[&others])?;
        let mut map: Vec<Option<usize>> = (0..old_bits).map(Some).collect();
        map.extend((0..v.width).map(|_| None));
        self.table.remap_inputs(&map);
        Ok(())
    }

    pub fn remove_input(&mut self, name: &str) -> Result<(), ModelError> {
        let idx = self
            .inputs
            .position(name)
            .ok_or_else(|| VarError::NotFound(name.to_string()))?;
        let first = self.inputs.first_bit(idx);
        let width = self.inputs.vars()[idx].width as usize;
        let total = self.inputs.bit_count();
        let map: Vec<Option<usize>> = (0..total)
            .filter(|c| *c < first || *c >= first + width)
            .map(Some)
            .collect();
        let bits = self.inputs.vars()[idx].bit_names();
        self.inputs.remove(name)?;
        self.table.remap_inputs(&map);
        for e in &mut self.exprs {
            if e.as_ref().is_some_and(|x| bits.iter().any(|b| x.uses(b))) {
                *e = None;
            }
        }
        Ok(())
    }

    /// Change an input's name and/or width. Expressions follow a pure rename; a changed width
    /// keeps the table entries of the bits that stay (counting from the least significant bit).
    pub fn replace_input(&mut self, old: &str, new: Var) -> Result<(), ModelError> {
        let idx = self
            .inputs
            .position(old)
            .ok_or_else(|| VarError::NotFound(old.to_string()))?;
        let old_var = self.inputs.vars()[idx].clone();
        let others = self.outputs.clone();
        self.inputs.replace(old, new.clone(), &[&others])?;
        let total_old = self.inputs.bit_count() + old_var.width as usize - new.width as usize;
        let first = self.inputs.first_bit(idx);
        // rebuild: columns before, the variable's new bits, columns after
        let mut full: Vec<Option<usize>> = (0..first).map(Some).collect();
        for k in 0..new.width {
            // k counts from the most significant new bit; match bits by weight (LSB aligned)
            let weight = new.width - 1 - k;
            full.push(
                (weight < old_var.width).then(|| first + (old_var.width - 1 - weight) as usize),
            );
        }
        full.extend((first + old_var.width as usize..total_old).map(Some));
        self.table.remap_inputs(&full);
        let old_bits = old_var.bit_names();
        let new_bits = new.bit_names();
        for e in &mut self.exprs {
            if let Some(x) = e {
                if old_bits.iter().any(|b| x.uses(b)) {
                    if old_bits.len() == new_bits.len() {
                        let mut y = x.clone();
                        // rename through temporaries so swapped names cannot collide
                        for (i, b) in old_bits.iter().enumerate() {
                            y = y.rename(b, &format!("\u{0}{i}"));
                        }
                        for (i, b) in new_bits.iter().enumerate() {
                            y = y.rename(&format!("\u{0}{i}"), b);
                        }
                        *x = y;
                    } else {
                        *e = None;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn move_input(&mut self, name: &str, delta: isize) -> Result<(), ModelError> {
        let idx = self
            .inputs
            .position(name)
            .ok_or_else(|| VarError::NotFound(name.to_string()))?;
        let before: Vec<(usize, usize)> = self
            .inputs
            .vars()
            .iter()
            .enumerate()
            .map(|(i, v)| (self.inputs.first_bit(i), v.width as usize))
            .collect();
        let new_idx = self.inputs.move_by(name, delta)?;
        let mut order: Vec<usize> = (0..before.len()).collect();
        let moved = order.remove(idx);
        order.insert(new_idx, moved);
        let map: Vec<Option<usize>> = order
            .iter()
            .flat_map(|o| (before[*o].0..before[*o].0 + before[*o].1).map(Some))
            .collect();
        self.table.remap_inputs(&map);
        Ok(())
    }

    pub fn add_output(&mut self, v: Var) -> Result<(), ModelError> {
        let others = self.inputs.clone();
        self.outputs.add(v.clone(), &[&others])?;
        for _ in 0..v.width {
            self.table.insert_output(self.table.output_count());
            self.exprs.push(None);
        }
        Ok(())
    }

    pub fn remove_output(&mut self, name: &str) -> Result<(), ModelError> {
        let idx = self
            .outputs
            .position(name)
            .ok_or_else(|| VarError::NotFound(name.to_string()))?;
        let first = self.outputs.first_bit(idx);
        let width = self.outputs.vars()[idx].width as usize;
        self.outputs.remove(name)?;
        for _ in 0..width {
            self.table.remove_output(first);
            self.exprs.remove(first);
        }
        Ok(())
    }

    /// Change an output's name and/or width (table columns of bits that stay are kept, aligned at
    /// the least significant bit).
    pub fn replace_output(&mut self, old: &str, new: Var) -> Result<(), ModelError> {
        let idx = self
            .outputs
            .position(old)
            .ok_or_else(|| VarError::NotFound(old.to_string()))?;
        let first = self.outputs.first_bit(idx);
        let old_w = self.outputs.vars()[idx].width as usize;
        let others = self.inputs.clone();
        self.outputs.replace(old, new.clone(), &[&others])?;
        let new_w = new.width as usize;
        let mut cols: Vec<Vec<Entry>> = vec![];
        let mut exprs: Vec<Option<Expr>> = vec![];
        for k in 0..new_w {
            let weight = new_w - 1 - k;
            if weight < old_w {
                let src = first + (old_w - 1 - weight);
                cols.push(self.table.column(src).to_vec());
                exprs.push(self.exprs[src].clone());
            } else {
                cols.push(vec![Entry::DontCare; self.table.row_count()]);
                exprs.push(None);
            }
        }
        for _ in 0..old_w {
            self.table.remove_output(first);
            self.exprs.remove(first);
        }
        for (k, (c, e)) in cols.into_iter().zip(exprs).enumerate() {
            self.table.insert_output(first + k);
            self.table.set_column(first + k, c);
            self.exprs.insert(first + k, e);
        }
        Ok(())
    }

    pub fn move_output(&mut self, name: &str, delta: isize) -> Result<(), ModelError> {
        let idx = self
            .outputs
            .position(name)
            .ok_or_else(|| VarError::NotFound(name.to_string()))?;
        let first = self.outputs.first_bit(idx);
        let width = self.outputs.vars()[idx].width as usize;
        let new_idx = self.outputs.move_by(name, delta)?;
        let new_first = self.outputs.first_bit(new_idx);
        let mut cols = vec![];
        let mut exprs = vec![];
        for _ in 0..width {
            cols.push(self.table.remove_output(first));
            exprs.push(self.exprs.remove(first));
        }
        for (k, (c, e)) in cols.into_iter().zip(exprs).enumerate() {
            self.table.insert_output(new_first + k);
            self.table.set_column(new_first + k, c);
            self.exprs.insert(new_first + k, e);
        }
        Ok(())
    }

    // ---------- table ----------

    /// Set one cell; the output's typed expression no longer matches the table, so it is dropped.
    pub fn set_entry(&mut self, row: usize, out: usize, e: Entry) {
        if self.table.get(row, out) != e {
            self.table.set(row, out, e);
            self.exprs[out] = None;
        }
    }

    /// Set every row of one output bit; only that output's typed expression is dropped.
    pub fn fill_output(&mut self, out: usize, e: Entry) {
        self.table.fill_column(out, e);
        self.exprs[out] = None;
    }

    /// Replace the whole table (same shape); all typed expressions are dropped.
    pub fn set_table(&mut self, table: TruthTable) {
        assert_eq!(table.input_count(), self.inputs.bit_count());
        assert_eq!(table.output_count(), self.outputs.bit_count());
        self.table = table;
        self.exprs = vec![None; self.outputs.bit_count()];
    }

    // ---------- expressions ----------

    /// The expression stored for an output bit, if one was set.
    pub fn stored_expression(&self, out: usize) -> Option<&Expr> {
        self.exprs[out].as_ref()
    }

    /// Set an output bit's expression; the table column is recomputed from it.
    pub fn set_expression(&mut self, out_bit: &str, e: Expr) -> Result<(), ModelError> {
        let out = self.output_index(out_bit)?;
        let names = self.input_bits();
        for v in e.variables() {
            if !names.contains(&v) {
                return Err(ModelError::UnknownVariable(v));
            }
        }
        self.table.set_column_from_expr(out, &e, &names);
        self.exprs[out] = Some(e);
        Ok(())
    }

    /// Read `text` (optionally `Out = expression`) for output bit `out_bit` and apply it.
    /// Returns the output bit it was applied to. Empty text clears the stored expression.
    pub fn set_expression_text(&mut self, out_bit: &str, text: &str) -> Result<String, ModelError> {
        let names = self.input_bits();
        let outs = self.output_bits();
        match parser::parse_assignment(text, &names, &outs)? {
            None => {
                let out = self.output_index(out_bit)?;
                self.exprs[out] = None;
                Ok(out_bit.to_string())
            }
            Some(p) => {
                let target = p.target.unwrap_or_else(|| out_bit.to_string());
                self.set_expression(&target, p.expr)?;
                Ok(target)
            }
        }
    }

    /// Minimal expression for an output bit (see [`implicant::minimize`]).
    pub fn minimal(&self, out: usize, form: Form, force: bool) -> Option<Minimal> {
        implicant::minimize(&self.table, out, form, force)
    }

    /// The expression to show for an output bit: the stored one, else the minimal sum of
    /// products (the unminimised one when the table is too large to minimise automatically).
    pub fn expression(&self, out: usize) -> Expr {
        if let Some(e) = &self.exprs[out] {
            return e.clone();
        }
        let names = self.input_bits();
        match self.minimal(out, Form::SumOfProducts, false) {
            Some(m) => m.to_expr(&names),
            None => implicant::canonical(&self.table, out, Form::SumOfProducts, &names),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(n: &str, w: u32) -> Var {
        Var::new(n, w)
    }

    fn model_ab_y() -> AnalyzerModel {
        let mut m = AnalyzerModel::new();
        m.add_input(v("a", 1)).unwrap();
        m.add_input(v("b", 1)).unwrap();
        m.add_output(v("y", 1)).unwrap();
        m
    }

    #[test]
    fn shapes_follow_the_variables() {
        let mut m = AnalyzerModel::new();
        m.add_input(v("A", 2)).unwrap();
        m.add_input(v("c", 1)).unwrap();
        m.add_output(v("Q", 3)).unwrap();
        assert_eq!(m.input_bits(), ["A[1]", "A[0]", "c"]);
        assert_eq!(m.output_bits(), ["Q[2]", "Q[1]", "Q[0]"]);
        assert_eq!((m.table().row_count(), m.table().output_count()), (8, 3));
    }

    #[test]
    fn inputs_and_outputs_must_have_different_names() {
        let mut m = model_ab_y();
        assert!(matches!(
            m.add_output(v("a", 1)),
            Err(ModelError::Var(VarError::Duplicate(_)))
        ));
        assert!(matches!(
            m.add_input(v("y", 1)),
            Err(ModelError::Var(VarError::Duplicate(_)))
        ));
    }

    #[test]
    fn expression_fills_the_table_and_back() {
        let mut m = model_ab_y();
        let applied = m.set_expression_text("y", "a ^ b").unwrap();
        assert_eq!(applied, "y");
        let col: String = m.table().column(0).iter().map(|e| e.to_char()).collect();
        assert_eq!(col, "0110");
        assert_eq!(m.expression(0).to_string(), "a ⊕ b");
        // editing a cell drops the typed expression; the minimal one is shown
        m.set_entry(3, 0, Entry::One);
        assert!(m.stored_expression(0).is_none());
        assert_eq!(m.expression(0).to_string(), "a + b");
    }

    #[test]
    fn assignment_syntax_picks_the_output() {
        let mut m = model_ab_y();
        m.add_output(v("z", 1)).unwrap();
        let t = m.set_expression_text("y", "z = a b").unwrap();
        assert_eq!(t, "z");
        assert!(m.stored_expression(0).is_none());
        assert_eq!(m.stored_expression(1).unwrap().to_string(), "a⋅b");
        m.set_expression_text("z", "").unwrap();
        assert!(m.stored_expression(1).is_none());
    }

    #[test]
    fn bad_expressions_are_rejected_without_changing_anything() {
        let mut m = model_ab_y();
        m.set_expression_text("y", "a b").unwrap();
        let before = m.clone();
        assert!(matches!(
            m.set_expression_text("y", "a +"),
            Err(ModelError::Parse(_))
        ));
        assert!(matches!(
            m.set_expression_text("y", "q"),
            Err(ModelError::Parse(_))
        ));
        assert!(matches!(
            m.set_expression("nope", Expr::var("a")),
            Err(ModelError::NoSuchOutput(_))
        ));
        assert!(matches!(
            m.set_expression("y", Expr::var("q")),
            Err(ModelError::UnknownVariable(_))
        ));
        assert_eq!(m, before);
    }

    #[test]
    fn filling_one_output_keeps_the_others_expressions() {
        let mut m = model_ab_y();
        m.add_output(v("z", 1)).unwrap();
        m.set_expression_text("y", "a b").unwrap();
        m.set_expression_text("z", "a").unwrap();
        m.fill_output(0, Entry::One);
        assert!(m.table().column(0).iter().all(|e| *e == Entry::One));
        assert!(m.stored_expression(0).is_none());
        assert_eq!(m.stored_expression(1).unwrap().to_string(), "a");
    }

    #[test]
    fn adding_an_input_keeps_existing_behaviour() {
        let mut m = model_ab_y();
        m.set_expression_text("y", "a b").unwrap();
        m.add_input(v("c", 1)).unwrap();
        // y does not depend on c: still a b
        assert_eq!(m.table().rows_with(0, Entry::One), [6, 7]);
        assert_eq!(m.expression(0).to_string(), "a⋅b");
    }

    #[test]
    fn removing_an_input_drops_dependent_expressions() {
        let mut m = model_ab_y();
        m.set_expression_text("y", "a + b").unwrap();
        m.remove_input("b").unwrap();
        assert_eq!(m.input_bits(), ["a"]);
        assert!(m.stored_expression(0).is_none());
        assert_eq!(m.table().row_count(), 2);
    }

    #[test]
    fn renaming_an_input_updates_expressions() {
        let mut m = model_ab_y();
        m.set_expression_text("y", "a ~b").unwrap();
        m.replace_input("a", v("x", 1)).unwrap();
        assert_eq!(m.stored_expression(0).unwrap().to_string(), "x⋅~b");
        // swapping names through two renames never merges them
        m.replace_input("x", v("b2", 1)).unwrap();
        m.replace_input("b", v("x", 1)).unwrap();
        assert_eq!(m.stored_expression(0).unwrap().to_string(), "b2⋅~x");
    }

    #[test]
    fn changing_an_input_width_keeps_the_low_bits() {
        let mut m = AnalyzerModel::new();
        m.add_input(v("A", 2)).unwrap();
        m.add_output(v("y", 1)).unwrap();
        // y = A[0]
        m.set_expression_text("y", "A[0]").unwrap();
        m.replace_input("A", v("A", 3)).unwrap();
        assert_eq!(m.input_bits(), ["A[2]", "A[1]", "A[0]"]);
        let want: Vec<usize> = (0..8).filter(|r| r & 1 == 1).collect();
        // the typed expression mentions A[0], which still exists as bit weight 0, but the width
        // changed so it was dropped; the table keeps the behaviour
        assert_eq!(m.table().rows_with(0, Entry::One), want);
    }

    #[test]
    fn moving_an_input_permutes_the_table() {
        let mut m = model_ab_y();
        m.set_expression_text("y", "a ~b").unwrap(); // row 0b10
        m.move_input("a", 1).unwrap();
        assert_eq!(m.input_bits(), ["b", "a"]);
        assert_eq!(m.table().rows_with(0, Entry::One), [0b01]);
        assert_eq!(m.expression(0).to_string(), "a⋅~b");
    }

    #[test]
    fn output_edits_keep_columns_with_their_bits() {
        let mut m = model_ab_y();
        m.add_output(v("z", 2)).unwrap();
        m.set_expression_text("y", "a").unwrap();
        m.set_expression_text("z[1]", "b").unwrap();
        m.set_expression_text("z[0]", "a b").unwrap();
        m.move_output("y", 1).unwrap();
        assert_eq!(m.output_bits(), ["z[1]", "z[0]", "y"]);
        assert_eq!(m.stored_expression(2).unwrap().to_string(), "a");
        assert_eq!(m.stored_expression(0).unwrap().to_string(), "b");
        m.replace_output("z", v("z", 3)).unwrap();
        assert_eq!(m.output_bits(), ["z[2]", "z[1]", "z[0]", "y"]);
        assert_eq!(m.stored_expression(1).unwrap().to_string(), "b");
        assert_eq!(m.stored_expression(2).unwrap().to_string(), "a⋅b");
        assert!(m.stored_expression(0).is_none());
        m.remove_output("z").unwrap();
        assert_eq!(m.output_bits(), ["y"]);
        assert_eq!(m.stored_expression(0).unwrap().to_string(), "a");
    }

    #[test]
    fn large_tables_are_not_minimised_unasked() {
        let mut m = AnalyzerModel::new();
        m.add_input(v("A", 7)).unwrap();
        m.add_output(v("y", 1)).unwrap();
        m.set_expression_text("y", "A[0]").unwrap();
        m.set_expression_text("y", "").unwrap();
        assert!(m.minimal(0, Form::SumOfProducts, false).is_none());
        assert_eq!(
            m.minimal(0, Form::SumOfProducts, true)
                .unwrap()
                .implicants
                .len(),
            1
        );
        // display falls back to the full list of minterms
        assert!(m.expression(0).size() > 100);
    }

    #[test]
    fn from_parts_checks_shapes_and_names() {
        let mut t = TruthTable::new(2, 1);
        t.fill_column(0, Entry::Zero);
        let m =
            AnalyzerModel::from_parts(&[v("a", 1), v("b", 1)], &[v("y", 1)], t.clone()).unwrap();
        assert_eq!(m.table(), &t);
        assert!(AnalyzerModel::from_parts(&[v("a", 1), v("a", 1)], &[v("y", 1)], t).is_err());
    }
}
