//! Everything the Combinational Analysis window does that is not drawing: editing variables,
//! the table and expressions, minimising, importing, exporting and building. The drawing code in
//! the sibling modules only reads this state and calls these methods, so all of it is testable.

use logisim_analyze::build::{build_circuit, gate_summary, BuildOptions};
use logisim_analyze::expr::{Expr, Notation};
use logisim_analyze::from_circuit::analyze_circuit;
use logisim_analyze::implicant::{Form, Minimal};
use logisim_analyze::kmap::{KMap, MAX_VARS};
use logisim_analyze::model::AnalyzerModel;
use logisim_analyze::parser::ParseError;
use logisim_analyze::table::Entry;
use logisim_analyze::var::Var;
use logisim_analyze::{csv, text};
use logisim_format::{CircuitDef, ProjectFile};

/// Which side of the table a variable belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Table,
    Expressions,
    Simplified,
}

/// What to show in the message line under the toolbar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Info(String),
    Error(String),
}

/// A simplified expression ready to show: the groups of rows and the expression they give.
#[derive(Debug, Clone, PartialEq)]
pub struct Simplified {
    pub minimal: Minimal,
    pub expr: Expr,
}

/// A typed expression that did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprProblem {
    pub text: String,
    pub error: ParseError,
}

pub struct Analyzer {
    pub model: AnalyzerModel,
    /// Increases on every change to the model; caches compare against it.
    rev: u64,
    pub tab: Tab,
    pub notation: Notation,
    /// Output bit shown in the Simplified tab.
    pub selected: usize,
    pub form: Form,
    /// Minimise even with more inputs than the automatic limit.
    pub forced: bool,
    pub message: Option<Message>,
    pub build: BuildOptions,
    /// Problems from typing expressions, by output bit.
    pub expr_problems: Vec<Option<ExprProblem>>,
    text_cache: Option<TextCache>,
    simp_cache: Option<SimpCache>,
}

struct TextCache {
    rev: u64,
    notation: Notation,
    lines: Vec<Option<String>>,
}

struct SimpCache {
    key: (u64, usize, Form, bool),
    value: Option<Simplified>,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        let mut a = Analyzer {
            model: AnalyzerModel::new(),
            rev: 0,
            tab: Tab::Table,
            notation: Notation::Mathematical,
            selected: 0,
            form: Form::SumOfProducts,
            forced: false,
            message: None,
            build: BuildOptions::default(),
            expr_problems: vec![],
            text_cache: None,
            simp_cache: None,
        };
        // a friendly starting point: two inputs and one output
        a.model.add_input(Var::new("A", 1)).unwrap();
        a.model.add_input(Var::new("B", 1)).unwrap();
        a.add_var(Side::Output, "Y").unwrap();
        a
    }

    #[cfg(test)]
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Record that the model changed: drops caches and resets selection bounds.
    fn changed(&mut self) {
        self.rev += 1;
        let n = self.model.table().output_count();
        self.expr_problems.resize(n, None);
        self.expr_problems.truncate(n);
        if self.selected >= n {
            self.selected = n.saturating_sub(1);
        }
    }

    fn fail(&mut self, e: impl ToString) {
        self.message = Some(Message::Error(e.to_string()));
    }
    fn info(&mut self, s: impl Into<String>) {
        self.message = Some(Message::Info(s.into()));
    }

    // ---------- variables ----------

    /// Add a variable typed as `Name` or `Name[7..0]`.
    pub fn add_var(&mut self, side: Side, text: &str) -> Result<(), String> {
        let v = Var::parse(text).map_err(|e| e.to_string())?;
        let (first, width) = (self.model.table().output_count(), v.width as usize);
        let r = match side {
            Side::Input => self.model.add_input(v),
            Side::Output => self.model.add_output(v),
        };
        r.map_err(|e| e.to_string())?;
        if side == Side::Output {
            // new outputs start at 0, so one click on a cell makes it 1
            for o in first..first + width {
                self.model.fill_output(o, Entry::Zero);
            }
        }
        self.changed();
        Ok(())
    }

    pub fn remove_var(&mut self, side: Side, name: &str) {
        let r = match side {
            Side::Input => self.model.remove_input(name),
            Side::Output => self.model.remove_output(name),
        };
        match r {
            Ok(()) => self.changed(),
            Err(e) => self.fail(e),
        }
    }

    pub fn replace_var(&mut self, side: Side, old: &str, new: Var) -> Result<(), String> {
        let r = match side {
            Side::Input => self.model.replace_input(old, new),
            Side::Output => self.model.replace_output(old, new),
        };
        r.map_err(|e| e.to_string())?;
        self.changed();
        Ok(())
    }

    pub fn move_var(&mut self, side: Side, name: &str, delta: isize) {
        let r = match side {
            Side::Input => self.model.move_input(name, delta),
            Side::Output => self.model.move_output(name, delta),
        };
        match r {
            Ok(()) => self.changed(),
            Err(e) => self.fail(e),
        }
    }

    // ---------- table ----------

    pub fn cycle_entry(&mut self, row: usize, out: usize) {
        let e = self.model.table().get(row, out).cycle();
        self.model.set_entry(row, out, e);
        self.changed();
    }

    pub fn fill_column(&mut self, out: usize, e: Entry) {
        self.model.fill_output(out, e);
        self.changed();
    }

    /// Set every row of every output to don't care.
    pub fn clear_table(&mut self) {
        for o in 0..self.model.table().output_count() {
            self.fill_column(o, Entry::DontCare);
        }
    }

    // ---------- expressions ----------

    /// The text to show for each output bit in the current notation. `None` means there is no
    /// expression to show (a table too big to simplify automatically).
    pub fn expression_texts(&mut self) -> &[Option<String>] {
        let stale = self
            .text_cache
            .as_ref()
            .is_none_or(|c| c.rev != self.rev || c.notation != self.notation);
        if stale {
            let n = self.model.table().output_count();
            let small = self.model.table().input_count() <= logisim_analyze::implicant::AUTO_LIMIT;
            let lines = (0..n)
                .map(|o| {
                    if self.model.stored_expression(o).is_some() || small {
                        Some(self.model.expression(o).to_text(self.notation))
                    } else {
                        None
                    }
                })
                .collect();
            self.text_cache = Some(TextCache {
                rev: self.rev,
                notation: self.notation,
                lines,
            });
        }
        &self.text_cache.as_ref().unwrap().lines
    }

    /// Apply typed expression text to output bit `out`. Text `Q = a b` goes to `Q` instead.
    pub fn set_expression_text(&mut self, out: usize, text: &str) {
        let names = self.model.output_bits();
        match self.model.set_expression_text(&names[out], text) {
            Ok(target) => {
                let idx = names.iter().position(|n| *n == target).unwrap_or(out);
                self.changed();
                self.expr_problems[out] = None;
                self.expr_problems[idx] = None;
            }
            Err(logisim_analyze::model::ModelError::Parse(e)) => {
                self.expr_problems[out] = Some(ExprProblem {
                    text: text.to_string(),
                    error: e,
                });
            }
            Err(e) => {
                self.expr_problems[out] = Some(ExprProblem {
                    text: text.to_string(),
                    error: ParseError {
                        message: e.to_string(),
                        offset: 0,
                        length: 0,
                    },
                });
            }
        }
    }

    /// Replace output `out`'s expression by its simplified form (see [`Self::simplified`]).
    pub fn use_simplified(&mut self, out: usize) {
        if let Some(s) = self.simplified(out) {
            let names = self.model.output_bits();
            if self.model.set_expression(&names[out], s.expr).is_ok() {
                self.changed();
                self.expr_problems[out] = None;
            }
        }
    }

    // ---------- simplifying ----------

    /// Number of inputs beyond which simplifying needs a click.
    pub fn auto_limit() -> usize {
        logisim_analyze::implicant::AUTO_LIMIT
    }

    pub fn needs_force(&self) -> bool {
        self.model.table().input_count() > Self::auto_limit() && !self.forced
    }

    /// Minimal form for output bit `out` (cached), or `None` when it has to be asked for.
    pub fn simplified(&mut self, out: usize) -> Option<Simplified> {
        if out >= self.model.table().output_count() {
            return None;
        }
        let key = (self.rev, out, self.form, self.forced);
        if self.simp_cache.as_ref().is_none_or(|c| c.key != key) {
            let value = self.model.minimal(out, self.form, self.forced).map(|m| {
                let expr = m.to_expr(&self.model.input_bits());
                Simplified { minimal: m, expr }
            });
            self.simp_cache = Some(SimpCache { key, value });
        }
        self.simp_cache.as_ref().unwrap().value.clone()
    }

    /// The Karnaugh map layout for the current inputs, if there are 1 to 6.
    pub fn kmap(&self) -> Option<KMap> {
        KMap::new(self.model.table().input_count())
    }

    pub fn kmap_limit() -> usize {
        MAX_VARS
    }

    // ---------- files ----------

    pub fn export_text(&self) -> String {
        text::export(&self.model)
    }
    pub fn export_csv(&self) -> String {
        csv::export(&self.model)
    }

    fn replace_model(&mut self, m: AnalyzerModel, what: impl Into<String>) {
        self.model = m;
        self.selected = 0;
        self.simp_cache = None;
        self.changed();
        self.info(what);
    }

    pub fn import_text(&mut self, content: &str) -> bool {
        match text::import(content) {
            Ok(m) => {
                self.replace_model(m, "Truth table imported.");
                true
            }
            Err(e) => {
                self.fail(e);
                false
            }
        }
    }

    pub fn import_csv(
        &mut self,
        content: &str,
        input_columns: Option<usize>,
    ) -> Result<(), String> {
        match csv::import(content, input_columns) {
            Ok(m) => {
                self.replace_model(m, "CSV imported.");
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Fill the window from the labelled pins of a circuit, by simulating it.
    pub fn load_from_circuit(&mut self, project: &ProjectFile, circuit: &str) -> bool {
        match analyze_circuit(project, circuit) {
            Ok(m) => {
                self.replace_model(m, format!("Truth table read from circuit '{circuit}'."));
                true
            }
            Err(e) => {
                self.fail(e);
                false
            }
        }
    }

    // ---------- building ----------

    /// Build the circuit; the second value summarises its gates.
    pub fn build_circuit(&self) -> Result<(CircuitDef, String), String> {
        let def = build_circuit(&self.model, &self.build).map_err(|e| e.to_string())?;
        let summary = gate_summary(&def);
        Ok((def, summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_xor(a: &mut Analyzer) {
        a.set_expression_text(0, "A ^ B");
    }

    #[test]
    fn starts_with_a_small_usable_table() {
        let a = Analyzer::new();
        assert_eq!(a.model.input_bits(), ["A", "B"]);
        assert_eq!(a.model.output_bits(), ["Y"]);
        assert_eq!(a.model.table().row_count(), 4);
    }

    #[test]
    fn adding_variables_validates_and_reports() {
        let mut a = Analyzer::new();
        a.add_var(Side::Input, "C").unwrap();
        a.add_var(Side::Output, "Q[1..0]").unwrap();
        assert_eq!(a.model.output_bits(), ["Y", "Q[1]", "Q[0]"]);
        assert_eq!(a.expr_problems.len(), 3);
        assert!(a
            .add_var(Side::Input, "A")
            .unwrap_err()
            .contains("already a variable named 'A'"));
        assert!(a
            .add_var(Side::Input, "1bad")
            .unwrap_err()
            .contains("not a valid variable"));
        assert!(a.add_var(Side::Input, "Y").is_err());
        a.remove_var(Side::Output, "Q");
        assert_eq!(a.model.output_bits(), ["Y"]);
    }

    #[test]
    fn rename_and_resize() {
        let mut a = Analyzer::new();
        a.replace_var(Side::Input, "A", Var::new("Data", 2))
            .unwrap();
        assert_eq!(a.model.input_bits(), ["Data[1]", "Data[0]", "B"]);
        assert!(a
            .replace_var(Side::Input, "B", Var::new("Data", 1))
            .is_err());
        a.move_var(Side::Input, "B", -1);
        assert_eq!(a.model.input_bits(), ["B", "Data[1]", "Data[0]"]);
    }

    #[test]
    fn clicking_cells_cycles_and_marks_the_table_as_edited() {
        let mut a = Analyzer::new();
        set_xor(&mut a);
        assert_eq!(a.model.table().get(3, 0), Entry::Zero);
        let r0 = a.rev();
        a.cycle_entry(3, 0);
        assert_eq!(a.model.table().get(3, 0), Entry::One);
        assert!(a.rev() > r0);
        assert!(a.model.stored_expression(0).is_none());
        a.cycle_entry(3, 0);
        assert_eq!(a.model.table().get(3, 0), Entry::DontCare);
        a.cycle_entry(3, 0);
        assert_eq!(a.model.table().get(3, 0), Entry::Zero);
    }

    #[test]
    fn filling_one_column_keeps_other_expressions() {
        let mut a = Analyzer::new();
        a.add_var(Side::Output, "Z").unwrap();
        a.set_expression_text(1, "A B");
        a.fill_column(0, Entry::One);
        assert!(a.model.table().column(0).iter().all(|e| *e == Entry::One));
        assert!(a.model.stored_expression(1).is_some());
        a.clear_table();
        assert!(a
            .model
            .table()
            .column(1)
            .iter()
            .all(|e| *e == Entry::DontCare));
    }

    #[test]
    fn typed_expressions_fill_the_table_and_errors_are_kept_with_position() {
        let mut a = Analyzer::new();
        set_xor(&mut a);
        let col: String = a
            .model
            .table()
            .column(0)
            .iter()
            .map(|e| e.to_char())
            .collect();
        assert_eq!(col, "0110");
        a.set_expression_text(0, "A + ");
        let p = a.expr_problems[0].as_ref().unwrap();
        assert_eq!(p.text, "A + ");
        assert!(p.error.message.contains("right"));
        // the model still has the last good expression
        assert!(a.model.stored_expression(0).is_some());
        a.set_expression_text(0, "A + B");
        assert!(a.expr_problems[0].is_none());
    }

    #[test]
    fn expression_texts_follow_the_notation_and_cache() {
        let mut a = Analyzer::new();
        set_xor(&mut a);
        assert_eq!(a.expression_texts()[0].as_deref(), Some("A ⊕ B"));
        a.notation = Notation::ProgBits;
        assert_eq!(a.expression_texts()[0].as_deref(), Some("A ^ B"));
    }

    #[test]
    fn big_tables_do_not_show_a_huge_default_expression() {
        let mut a = Analyzer::new();
        a.replace_var(Side::Input, "A", Var::new("A", 8)).unwrap();
        assert_eq!(a.expression_texts()[0], None);
        assert!(a.needs_force());
        assert!(a.simplified(0).is_none());
        a.forced = true;
        assert!(a.simplified(0).is_some());
        assert!(!a.needs_force());
    }

    #[test]
    fn simplified_form_and_use_it() {
        let mut a = Analyzer::new();
        a.set_expression_text(0, "A B + A ~B"); // = A
        let s = a.simplified(0).unwrap();
        assert_eq!(s.expr.to_text(Notation::Mathematical), "A");
        a.use_simplified(0);
        assert_eq!(a.model.stored_expression(0).unwrap().to_string(), "A");
        a.form = Form::ProductOfSums;
        assert_eq!(a.simplified(0).unwrap().expr.to_string(), "A");
    }

    #[test]
    fn kmap_availability() {
        let mut a = Analyzer::new();
        assert!(a.kmap().is_some());
        a.replace_var(Side::Input, "A", Var::new("A", 6)).unwrap();
        assert!(a.kmap().is_none()); // 7 inputs
    }

    #[test]
    fn text_and_csv_round_trip_through_the_state() {
        let mut a = Analyzer::new();
        a.set_expression_text(0, "A ^ B");
        let t = a.export_text();
        let c = a.export_csv();
        let mut b = Analyzer::new();
        assert!(b.import_text(&t));
        assert_eq!(b.model.table(), a.model.table());
        let mut d = Analyzer::new();
        d.import_csv(&c, None).unwrap();
        assert_eq!(d.model.table(), a.model.table());
        assert!(!b.import_text("garbage"));
        assert!(matches!(b.message, Some(Message::Error(_))));
        assert!(d.import_csv("a\n", None).is_err());
    }

    #[test]
    fn build_returns_a_circuit_and_summary() {
        let mut a = Analyzer::new();
        a.set_expression_text(0, "A B");
        let (def, summary) = a.build_circuit().unwrap();
        assert_eq!(summary, "1 AND");
        assert_eq!(def.name, "Analyzed");
        a.build.nand_only = true;
        assert_eq!(a.build_circuit().unwrap().1, "2 NAND");
    }

    #[test]
    fn load_from_circuit_reads_pins() {
        let mut src = Analyzer::new();
        src.set_expression_text(0, "A + B");
        let (def, _) = src.build_circuit().unwrap();
        let mut p = ProjectFile::new("p", &def.name);
        p.circuits.push(def.clone());
        let mut a = Analyzer::new();
        assert!(a.load_from_circuit(&p, &def.name));
        assert_eq!(a.model.table(), src.model.table());
        assert!(!a.load_from_circuit(&p, "missing"));
    }
}
