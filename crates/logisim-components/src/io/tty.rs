//! TTY: a clocked terminal that appends the 7-bit character on its input each active edge.
//!
//! Port order: 0 = clear (20,10); 1 = clock (0,0); 2 = write-enable (10,10); 3 = 7-bit data (0,-10).
//! The UI reads the screen through [`Tty::handle`].

use std::cell::RefCell;
use std::rc::Rc;

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

use super::{EdgeTrigger, EDGES};

/// Screen contents and cursor of a TTY (Logisim's `TtyState`).
#[derive(Debug, Clone)]
pub struct TtyState {
    last_clock: Value,
    /// Completed rows; one fewer than the screen height (the last row is `last_row`).
    row_data: Vec<String>,
    cols: usize,
    last_row: String,
    row: usize,
}

impl TtyState {
    pub fn new(rows: usize, cols: usize) -> Self {
        TtyState { last_clock: Value::UNKNOWN, row_data: vec![String::new(); rows - 1], cols, last_row: String::new(), row: 0 }
    }

    pub fn clear(&mut self) {
        self.row_data.iter_mut().for_each(String::clear);
        self.last_row.clear();
        self.row = 0;
    }

    /// Append one character; handles form feed, backspace and newline like Logisim.
    pub fn add(&mut self, c: char) {
        match c {
            '\u{c}' => self.clear(),
            '\u{8}' => {
                self.last_row.pop();
            }
            '\n' | '\r' => self.commit(),
            c if !c.is_control() => {
                if self.last_row.chars().count() == self.cols {
                    self.commit();
                }
                self.last_row.push(c);
            }
            _ => {}
        }
    }

    fn commit(&mut self) {
        if self.row >= self.row_data.len() {
            if !self.row_data.is_empty() {
                self.row_data.remove(0);
                self.row_data.push(std::mem::take(&mut self.last_row));
            } else {
                self.last_row.clear();
            }
        } else {
            self.row_data[self.row] = std::mem::take(&mut self.last_row);
            self.row += 1;
        }
        self.last_row.clear();
    }

    pub fn rows(&self) -> usize {
        self.row_data.len() + 1
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn cursor_row(&self) -> usize {
        self.row
    }
    pub fn cursor_col(&self) -> usize {
        self.last_row.chars().count()
    }
    /// Text of screen row `index` (empty below the cursor row).
    pub fn row_string(&self, index: usize) -> String {
        match index.cmp(&self.row) {
            std::cmp::Ordering::Less => self.row_data[index].clone(),
            std::cmp::Ordering::Equal => self.last_row.clone(),
            std::cmp::Ordering::Greater => String::new(),
        }
    }

    /// Resize, keeping as much text as fits (Logisim's `updateSize`).
    pub fn update_size(&mut self, rows: usize, cols: usize) {
        let old_rows = self.rows();
        if rows != old_rows {
            let mut new_data = vec![String::new(); rows - 1];
            if rows > old_rows || self.row < rows - 1 {
                let n = self.row.min(rows - 1);
                new_data[..n].clone_from_slice(&self.row_data[..n]);
            } else {
                let skip = self.row + 1 - rows;
                for (i, slot) in new_data.iter_mut().enumerate() {
                    *slot = self.row_data[skip + i].clone();
                }
                self.row = rows - 1;
            }
            self.row_data = new_data;
        }
        if cols != self.cols {
            if cols < self.cols {
                for s in &mut self.row_data {
                    *s = s.chars().take(cols).collect();
                }
                self.last_row = self.last_row.chars().take(cols).collect();
            }
            self.cols = cols;
        }
    }
}

pub type TtyHandle = Rc<RefCell<TtyState>>;

pub struct Tty {
    rows: usize,
    cols: usize,
    trigger: EdgeTrigger,
    state: TtyHandle,
}

impl Tty {
    /// `rows` 1..=48, `cols` 1..=120.
    pub fn new(rows: usize, cols: usize) -> Self {
        assert!((1..=48).contains(&rows) && (1..=120).contains(&cols), "TTY size out of range");
        Tty { rows, cols, trigger: EdgeTrigger::Rising, state: Rc::new(RefCell::new(TtyState::new(rows, cols))) }
    }
    pub fn handle(&self) -> TtyHandle {
        Rc::clone(&self.state)
    }
}

impl Component for Tty {
    fn name(&self) -> &str {
        "TTY"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new()
            .with("rows", self.rows as i64)
            .with("cols", self.cols as i64)
            .with("trigger", OptionMap::name_of(self.trigger, &EDGES))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("rows", "Rows", 1, 48, 8),
            OptionSpec::int("cols", "Columns", 1, 120, 32),
            OptionSpec::choice("trigger", "Clock trigger", &["rising", "falling"], "rising"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["rows", "cols", "trigger"])?;
        if let Some(v) = m.int("rows", 1, 48)? {
            self.rows = v as usize;
        }
        if let Some(v) = m.int("cols", 1, 120)? {
            self.cols = v as usize;
        }
        self.trigger = m.choice("trigger", &EDGES)?.unwrap_or(self.trigger);
        self.state.borrow_mut().update_size(self.rows, self.cols);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![
            Port::new(20, 10, PortKind::Input, BitWidth::ONE),
            Port::new(0, 0, PortKind::Input, BitWidth::ONE),
            Port::new(10, 10, PortKind::Input, BitWidth::ONE),
            Port::new(0, -10, PortKind::Input, BitWidth::new(7)),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let clear = ctx.port_value(0);
        let clock = ctx.port_value(1);
        let enable = ctx.port_value(2);
        let data = ctx.port_value(3);
        let mut st = self.state.borrow_mut();
        let last = std::mem::replace(&mut st.last_clock, clock);
        if clear == Value::TRUE {
            st.clear();
        } else if enable != Value::FALSE && self.trigger.fired(last, clock) {
            st.add(data.to_u64().map_or('?', |v| v as u8 as char));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::testutil::Rig;

    fn send(r: &mut Rig, c: char) {
        r.set(3, Value::known(BitWidth::new(7), c as u64));
        r.set(1, Value::TRUE);
        r.set(1, Value::FALSE);
    }

    fn rig(rows: usize, cols: usize) -> (Rig, TtyHandle) {
        let t = Tty::new(rows, cols);
        let h = t.handle();
        let r = Rig::new(
            Box::new(t),
            &[(0, Value::FALSE), (1, Value::FALSE), (2, Value::TRUE), (3, Value::known(BitWidth::new(7), 0))],
        );
        (r, h)
    }

    #[test]
    fn types_wraps_and_scrolls() {
        let (mut r, h) = rig(3, 4);
        for c in "abcdef".chars() {
            send(&mut r, c);
        }
        assert_eq!((h.borrow().row_string(0), h.borrow().row_string(1)), ("abcd".to_string(), "ef".to_string()));
        for c in "\ngh\nij".chars() {
            send(&mut r, c);
        }
        // 3 rows total: the oldest row ("abcd") scrolled away
        let s = h.borrow();
        assert_eq!((s.row_string(0), s.row_string(1), s.row_string(2)), ("ef".into(), "gh".into(), "ij".into()));
        assert_eq!(s.cursor_row(), 2);
    }

    #[test]
    fn backspace_form_feed_and_clear() {
        let (mut r, h) = rig(4, 8);
        for c in "hey!".chars() {
            send(&mut r, c);
        }
        send(&mut r, '\u{8}');
        assert_eq!(h.borrow().row_string(0), "hey");
        send(&mut r, '\u{c}');
        assert_eq!(h.borrow().row_string(0), "");
        send(&mut r, 'x');
        r.set(0, Value::TRUE);
        assert_eq!(h.borrow().row_string(0), "");
    }

    #[test]
    fn disabled_and_falling_edge() {
        let (mut r, h) = rig(2, 8);
        r.set(2, Value::FALSE);
        send(&mut r, 'a');
        assert_eq!(h.borrow().row_string(0), "");
        r.set(2, Value::TRUE);
        let mut t = Tty::new(2, 8);
        t.set_options(&OptionMap::new().with("trigger", "falling")).unwrap();
        let h2 = t.handle();
        let mut r2 = Rig::new(
            Box::new(t),
            &[(0, Value::FALSE), (1, Value::FALSE), (2, Value::TRUE), (3, Value::known(BitWidth::new(7), b'z' as u64))],
        );
        r2.set(1, Value::TRUE);
        assert_eq!(h2.borrow().row_string(0), "");
        r2.set(1, Value::FALSE);
        assert_eq!(h2.borrow().row_string(0), "z");
    }

    #[test]
    fn resize_keeps_text() {
        let mut s = TtyState::new(4, 10);
        for c in "one\ntwo\nthree".chars() {
            s.add(c);
        }
        s.update_size(2, 4);
        assert_eq!((s.rows(), s.cols()), (2, 4));
        assert_eq!(s.row_string(0), "two");
        assert_eq!(s.row_string(1), "thre");
    }
}
