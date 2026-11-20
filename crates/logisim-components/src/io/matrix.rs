//! LED matrix and LED bar: a grid of LEDs driven by columns, rows, or row/column select lines.
//!
//! LED Matrix ports (east-facing, y grows downward):
//! - column input (default): port `i` = column i (`rows` bits) at (10*i, 0) (or above the grid if
//!   `select_loc` is top-right);
//! - row input: port `i` = row i (`cols` bits) at (0, 10*i) (or right of the grid);
//! - select input: port 0 = column bits (`cols`), port 1 = row-select bits (`rows`).
//!
//! The LED Bar is a 1-row matrix with its own options (`segments`, `height`).
//! The UI reads lit cells through [`MatrixState::get`], which includes persistence.

use std::cell::RefCell;
use std::rc::Rc;

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

/// Which side the inputs are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectLoc {
    #[default]
    BottomLeft,
    TopRight,
}

/// How the matrix is wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatrixInput {
    /// One input per column (default).
    #[default]
    Column,
    /// One input per row.
    Row,
    /// A column-bits bus plus a row-select bus.
    Select,
}

const INPUTS: [(&str, MatrixInput); 3] = [("column", MatrixInput::Column), ("row", MatrixInput::Row), ("select", MatrixInput::Select)];
const SELECT_LOCS: [(&str, SelectLoc); 2] = [("bottom_left", SelectLoc::BottomLeft), ("top_right", SelectLoc::TopRight)];

#[derive(Debug, Clone)]
pub struct MatrixOptions {
    pub rows: u8,
    pub cols: u8,
    pub input: MatrixInput,
    pub select_loc: SelectLoc,
    /// How many ticks a lit dot stays lit after it is driven low.
    pub persist: u32,
    /// Vertical port spacing factor (used by the LED bar's height).
    pub scale_y: i32,
}

impl Default for MatrixOptions {
    fn default() -> Self {
        MatrixOptions { rows: 7, cols: 5, input: MatrixInput::Column, select_loc: SelectLoc::BottomLeft, persist: 0, scale_y: 1 }
    }
}

impl MatrixOptions {
    pub fn rows(mut self, r: u8) -> Self {
        assert!((1..=64).contains(&r));
        self.rows = r;
        self
    }
    pub fn cols(mut self, c: u8) -> Self {
        assert!((1..=64).contains(&c));
        self.cols = c;
        self
    }
    pub fn input(mut self, i: MatrixInput) -> Self {
        self.input = i;
        self
    }
    pub fn select_loc(mut self, l: SelectLoc) -> Self {
        self.select_loc = l;
        self
    }
    pub fn persist(mut self, ticks: u32) -> Self {
        self.persist = ticks;
        self
    }
}

/// Dot values and persistence (Logisim's `DotMatrixBase.State`).
#[derive(Debug, Clone)]
pub struct MatrixState {
    rows: usize,
    cols: usize,
    grid: Vec<Value>,
    persist_to: Vec<i64>,
    tick: i64,
}

impl MatrixState {
    pub fn new(rows: usize, cols: usize) -> Self {
        MatrixState { rows, cols, grid: vec![Value::UNKNOWN; rows * cols], persist_to: vec![-1; rows * cols], tick: 0 }
    }
    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    /// Value shown by the dot at (`row`, `col`); a recently-lit dot still reads high while it persists.
    pub fn get(&self, row: usize, col: usize) -> Value {
        let i = row * self.cols + col;
        if self.grid[i] == Value::FALSE && self.persist_to[i] - self.tick >= 0 {
            Value::TRUE
        } else {
            self.grid[i]
        }
    }
    /// Convenience: is the dot lit?
    pub fn is_lit(&self, row: usize, col: usize) -> bool {
        self.get(row, col) == Value::TRUE
    }

    fn set_cell(&mut self, i: usize, val: Value, persist: i64) {
        if self.grid[i] == Value::TRUE {
            self.persist_to[i] = persist - 1;
        }
        self.grid[i] = val;
        if val == Value::TRUE {
            self.persist_to[i] = persist;
        }
    }
    fn set_column(&mut self, index: usize, v: Value, persist: i64) {
        let mut loc = (self.rows - 1) * self.cols + index;
        for (i, b) in v.bits().into_iter().enumerate() {
            self.set_cell(loc, b, persist);
            if i + 1 < self.rows {
                loc -= self.cols;
            }
        }
    }
    fn set_row(&mut self, index: usize, v: Value, persist: i64) {
        let mut loc = (index + 1) * self.cols - 1;
        for (i, b) in v.bits().into_iter().enumerate() {
            self.set_cell(loc, b, persist);
            if i + 1 < self.cols {
                loc -= 1;
            }
        }
    }
    fn set_select(&mut self, rows: Value, cols: Value, persist: i64) {
        let (rv, cv) = (rows.bits(), cols.bits());
        let mut loc = 0;
        for whole_row in rv.into_iter().rev() {
            if whole_row == Value::TRUE {
                for c in &cv {
                    self.set_cell(loc, *c, persist);
                    loc += 1;
                }
            } else {
                let w = if whole_row == Value::FALSE { Value::FALSE } else { Value::ERROR };
                for _ in &cv {
                    if self.grid[loc] == Value::TRUE {
                        self.persist_to[loc] = persist - 1;
                    }
                    self.grid[loc] = w;
                    loc += 1;
                }
            }
        }
    }
}

pub type MatrixHandle = Rc<RefCell<MatrixState>>;

pub struct LedMatrix {
    opts: MatrixOptions,
    state: MatrixHandle,
}

impl LedMatrix {
    pub fn new(opts: MatrixOptions) -> Self {
        let state = Rc::new(RefCell::new(MatrixState::new(opts.rows as usize, opts.cols as usize)));
        LedMatrix { opts, state }
    }
    pub fn handle(&self) -> MatrixHandle {
        Rc::clone(&self.state)
    }

    fn resize_state(&mut self) {
        let (r, c) = (self.opts.rows as usize, self.opts.cols as usize);
        let mut s = self.state.borrow_mut();
        if s.rows != r || s.cols != c {
            let tick = s.tick;
            *s = MatrixState::new(r, c);
            s.tick = tick;
            s.persist_to.iter_mut().for_each(|p| *p = tick - 1);
        }
    }

    fn matrix_ports(&self) -> Vec<Port> {
        let o = &self.opts;
        let (rows, cols) = (o.rows as i32, o.cols as i32);
        let bl = o.select_loc == SelectLoc::BottomLeft;
        let (rw, cw) = (BitWidth::new(o.rows), BitWidth::new(o.cols));
        match o.input {
            MatrixInput::Column => (0..cols)
                .map(|i| Port::new(10 * i, if bl { 0 } else { -rows * 10 * o.scale_y }, PortKind::Input, rw))
                .collect(),
            MatrixInput::Row => (0..rows).map(|i| Port::new(if bl { 0 } else { cols * 10 }, 10 * i, PortKind::Input, cw)).collect(),
            MatrixInput::Select if rows <= 1 => {
                vec![Port::new(0, 0, PortKind::Input, cw), Port::new(10 * cols, 0, PortKind::Input, rw)]
            }
            MatrixInput::Select => {
                let dx = if bl { 0 } else { cols * 10 };
                vec![Port::new(dx, 0, PortKind::Input, cw), Port::new(dx, 10, PortKind::Input, rw)]
            }
        }
    }

    fn drive(&mut self, ctx: &mut dyn PropagateCtx) {
        let o = &self.opts;
        let mut s = self.state.borrow_mut();
        let persist = s.tick + o.persist as i64;
        match o.input {
            MatrixInput::Row => {
                for i in 0..o.rows as usize {
                    s.set_row(i, ctx.port_value(i), persist);
                }
            }
            MatrixInput::Column => {
                for i in 0..o.cols as usize {
                    s.set_column(i, ctx.port_value(i), persist);
                }
            }
            MatrixInput::Select => s.set_select(ctx.port_value(1), ctx.port_value(0), persist),
        }
    }
}

impl Component for LedMatrix {
    fn name(&self) -> &str {
        "LED Matrix"
    }
    fn options(&self) -> OptionMap {
        let o = &self.opts;
        OptionMap::new()
            .with("rows", o.rows)
            .with("cols", o.cols)
            .with("input", OptionMap::name_of(o.input, &INPUTS))
            .with("select_loc", OptionMap::name_of(o.select_loc, &SELECT_LOCS))
            .with("persist", o.persist as i64)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("rows", "Rows", 1, 64, 7),
            OptionSpec::int("cols", "Columns", 1, 64, 5),
            OptionSpec::choice("input", "Input format", &["column", "row", "select"], "column"),
            OptionSpec::choice("select_loc", "Input side", &["bottom_left", "top_right"], "bottom_left"),
            OptionSpec::int("persist", "Persistence (ticks)", 0, u32::MAX as i64, 0),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["rows", "cols", "input", "select_loc", "persist"])?;
        let o = &mut self.opts;
        if let Some(v) = m.int("rows", 1, 64)? {
            o.rows = v as u8;
        }
        if let Some(v) = m.int("cols", 1, 64)? {
            o.cols = v as u8;
        }
        o.input = m.choice("input", &INPUTS)?.unwrap_or(o.input);
        o.select_loc = m.choice("select_loc", &SELECT_LOCS)?.unwrap_or(o.select_loc);
        if let Some(v) = m.int("persist", 0, u32::MAX as i64)? {
            o.persist = v as u32;
        }
        self.resize_state();
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        self.matrix_ports()
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        self.drive(ctx);
    }
    fn tick(&mut self, _ctx: &mut dyn PropagateCtx) {
        self.state.borrow_mut().tick += 1;
    }
}

const BAR_INPUTS: [(&str, MatrixInput); 2] = [("separated", MatrixInput::Column), ("one_wire", MatrixInput::Row)];

/// LED bar: `segments` LEDs in one row, wired separately or as a single bus.
pub struct LedBar {
    inner: LedMatrix,
    height: i32,
}

impl LedBar {
    pub const DEFAULT_HEIGHT: i32 = 3;

    pub fn new() -> Self {
        let opts = MatrixOptions { rows: 1, cols: 8, input: MatrixInput::Column, scale_y: Self::DEFAULT_HEIGHT, ..MatrixOptions::default() };
        LedBar { inner: LedMatrix::new(opts), height: Self::DEFAULT_HEIGHT }
    }
    pub fn handle(&self) -> MatrixHandle {
        self.inner.handle()
    }
}

impl Default for LedBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for LedBar {
    fn name(&self) -> &str {
        "LED Bar"
    }
    fn options(&self) -> OptionMap {
        let o = &self.inner.opts;
        OptionMap::new()
            .with("segments", o.cols)
            .with("input", OptionMap::name_of(o.input, &BAR_INPUTS))
            .with("height", self.height as i64)
            .with("select_loc", OptionMap::name_of(o.select_loc, &SELECT_LOCS))
            .with("persist", o.persist as i64)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("segments", "Segments", 1, 64, 8),
            OptionSpec::choice("input", "Input format", &["separated", "one_wire"], "separated"),
            OptionSpec::int("height", "Bar height", 1, 8, Self::DEFAULT_HEIGHT as i64),
            OptionSpec::choice("select_loc", "Input side", &["bottom_left", "top_right"], "bottom_left"),
            OptionSpec::int("persist", "Persistence (ticks)", 0, u32::MAX as i64, 0),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["segments", "input", "height", "select_loc", "persist"])?;
        let o = &mut self.inner.opts;
        if let Some(v) = m.int("segments", 1, 64)? {
            o.cols = v as u8;
        }
        o.input = m.choice("input", &BAR_INPUTS)?.unwrap_or(o.input);
        if let Some(v) = m.int("height", 1, 8)? {
            self.height = v as i32;
            o.scale_y = self.height;
        }
        o.select_loc = m.choice("select_loc", &SELECT_LOCS)?.unwrap_or(o.select_loc);
        if let Some(v) = m.int("persist", 0, u32::MAX as i64)? {
            o.persist = v as u32;
        }
        self.inner.resize_state();
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        self.inner.matrix_ports()
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        self.inner.drive(ctx);
    }
    fn tick(&mut self, ctx: &mut dyn PropagateCtx) {
        self.inner.tick(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::testutil::Rig;

    fn bits(w: u8, v: u64) -> Value {
        Value::known(BitWidth::new(w), v)
    }

    #[test]
    fn column_input_lights_dots_bottom_up() {
        let m = LedMatrix::new(MatrixOptions::default().rows(3).cols(2));
        let h = m.handle();
        // column 0 = 0b001 (bottom row), column 1 = 0b100 (top row)
        let _r = Rig::new(Box::new(m), &[(0, bits(3, 0b001)), (1, bits(3, 0b100))]);
        let s = h.borrow();
        let lit: Vec<_> = (0..3).flat_map(|r| (0..2).map(move |c| (r, c))).filter(|&(r, c)| s.is_lit(r, c)).collect();
        assert_eq!(lit, [(0, 1), (2, 0)]);
    }

    #[test]
    fn row_input_msb_is_leftmost_column() {
        let m = LedMatrix::new(MatrixOptions::default().rows(2).cols(4).input(MatrixInput::Row));
        let h = m.handle();
        let _r = Rig::new(Box::new(m), &[(0, bits(4, 0b1000)), (1, bits(4, 0b0001))]);
        let s = h.borrow();
        assert!(s.is_lit(0, 0) && !s.is_lit(0, 3));
        assert!(s.is_lit(1, 3) && !s.is_lit(1, 0));
    }

    #[test]
    fn select_input_scans_rows() {
        let m = LedMatrix::new(MatrixOptions::default().rows(2).cols(3).input(MatrixInput::Select));
        let h = m.handle();
        // row-select 0b10 -> first (top) row takes the column bits, other row is cleared
        let mut r = Rig::new(Box::new(m), &[(0, bits(3, 0b101)), (1, bits(2, 0b10))]);
        {
            let s = h.borrow();
            assert_eq!([s.get(0, 0), s.get(0, 1), s.get(0, 2)], [Value::TRUE, Value::FALSE, Value::TRUE]);
            assert!((0..3).all(|c| s.get(1, c) == Value::FALSE));
        }
        r.set(1, Value::unknown(BitWidth::new(2)));
        assert_eq!(h.borrow().get(0, 0), Value::ERROR);
    }

    #[test]
    fn persistence_keeps_dots_lit_for_a_while() {
        let m = LedMatrix::new(MatrixOptions::default().rows(1).cols(1).persist(2));
        let h = m.handle();
        let mut r = Rig::new(Box::new(m), &[(0, Value::TRUE)]);
        assert!(h.borrow().is_lit(0, 0));
        r.set(0, Value::FALSE);
        assert!(h.borrow().is_lit(0, 0), "still lit right after going low");
        r.tick();
        r.tick();
        r.tick();
        assert!(!h.borrow().is_lit(0, 0));
    }

    #[test]
    fn matrix_port_layout() {
        let p = |o: MatrixOptions| -> Vec<(i32, i32, u8)> {
            LedMatrix::new(o).ports().iter().map(|p| (p.offset.x, p.offset.y, p.width.get())).collect()
        };
        assert_eq!(p(MatrixOptions::default().rows(2).cols(2)), [(0, 0, 2), (10, 0, 2)]);
        assert_eq!(p(MatrixOptions::default().rows(2).cols(3).input(MatrixInput::Row)), [(0, 0, 3), (0, 10, 3)]);
        assert_eq!(p(MatrixOptions::default().rows(2).cols(3).input(MatrixInput::Select)), [(0, 0, 3), (0, 10, 2)]);
        assert_eq!(p(MatrixOptions::default().rows(1).cols(4).input(MatrixInput::Select)), [(0, 0, 4), (40, 0, 1)]);
    }

    #[test]
    fn led_bar_separated_and_one_wire() {
        let mut bar = LedBar::new();
        bar.set_options(&OptionMap::new().with("segments", 3)).unwrap();
        let h = bar.handle();
        let _r = Rig::new(Box::new(bar), &[(0, Value::TRUE), (1, Value::FALSE), (2, Value::TRUE)]);
        let s = h.borrow();
        assert_eq!([s.is_lit(0, 0), s.is_lit(0, 1), s.is_lit(0, 2)], [true, false, true]);
        drop(s);

        let mut bar = LedBar::new();
        bar.set_options(&OptionMap::new().with("segments", 4).with("input", "one_wire")).unwrap();
        assert_eq!(bar.ports().len(), 1);
        let h = bar.handle();
        let _r = Rig::new(Box::new(bar), &[(0, bits(4, 0b0110))]);
        let s = h.borrow();
        // MSB is the leftmost segment
        assert_eq!([s.is_lit(0, 0), s.is_lit(0, 1), s.is_lit(0, 2), s.is_lit(0, 3)], [false, true, true, false]);
    }

    #[test]
    fn resizing_resets_the_grid() {
        let mut m = LedMatrix::new(MatrixOptions::default());
        let h = m.handle();
        m.set_options(&OptionMap::new().with("rows", 2).with("cols", 2)).unwrap();
        assert_eq!((h.borrow().rows(), h.borrow().cols()), (2, 2));
    }
}
