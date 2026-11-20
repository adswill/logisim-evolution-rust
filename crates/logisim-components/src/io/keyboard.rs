//! Keyboard: a typed-character buffer that releases one character per active clock edge.
//!
//! Port order: 0 = clear (20,10); 1 = clock (0,0); 2 = read-enable (10,10);
//! 3 = char-available (130,10, out); 4 = 7-bit character (140,10, out).
//! The UI feeds keystrokes through [`Keyboard::handle`] and then calls `Circuit::refresh(id)`.

use std::cell::RefCell;
use std::rc::Rc;

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

use super::{EdgeTrigger, EDGES};

const DELAY_CHAR: u32 = 9;
const DELAY_AVAILABLE: u32 = 11;

/// Keystroke buffer with an editing cursor (Logisim's `KeyboardData`, minus layout code).
#[derive(Debug, Clone)]
pub struct KeyboardData {
    last_clock: Value,
    buffer: Vec<char>,
    capacity: usize,
    cursor: usize,
}

impl KeyboardData {
    pub fn new(capacity: usize) -> Self {
        KeyboardData { last_clock: Value::UNKNOWN, buffer: Vec::new(), capacity, cursor: 0 }
    }
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }
    /// Insert at the cursor; false if the buffer is full.
    pub fn insert(&mut self, c: char) -> bool {
        if self.buffer.len() >= self.capacity {
            return false;
        }
        self.buffer.insert(self.cursor, c);
        self.cursor += 1;
        true
    }
    /// Type a character as the keyboard would: printable characters, backspace, newline and
    /// form feed are inserted; other control characters are ignored. Returns true if the buffer changed.
    pub fn type_char(&mut self, c: char) -> bool {
        if !c.is_control() || matches!(c, '\u{8}' | '\n' | '\u{c}') {
            self.insert(c)
        } else {
            false
        }
    }
    /// Delete the character under the cursor (the Delete key).
    pub fn delete(&mut self) -> bool {
        if self.cursor >= self.buffer.len() {
            return false;
        }
        self.buffer.remove(self.cursor);
        true
    }
    /// Remove and return the oldest character (`'\0'` if empty).
    pub fn dequeue(&mut self) -> char {
        if self.buffer.is_empty() {
            return '\0';
        }
        let c = self.buffer.remove(0);
        self.cursor = self.cursor.saturating_sub(1);
        c
    }
    pub fn move_cursor_by(&mut self, delta: isize) -> bool {
        match self.cursor.checked_add_signed(delta) {
            Some(p) if p <= self.buffer.len() => {
                self.cursor = p;
                true
            }
            _ => false,
        }
    }
    /// Move the cursor, clamped to the end of the buffer.
    pub fn set_cursor(&mut self, pos: usize) -> bool {
        let pos = pos.min(self.buffer.len());
        let changed = pos != self.cursor;
        self.cursor = pos;
        changed
    }
    pub fn char_at(&self, pos: usize) -> char {
        self.buffer.get(pos).copied().unwrap_or('\0')
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn text(&self) -> String {
        self.buffer.iter().collect()
    }
    pub fn len(&self) -> usize {
        self.buffer.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

pub type KeyboardHandle = Rc<RefCell<KeyboardData>>;

pub struct Keyboard {
    buffer_len: usize,
    trigger: EdgeTrigger,
    data: KeyboardHandle,
}

impl Keyboard {
    /// `buffer_len` is 1..=256 (Logisim default 32).
    pub fn new(buffer_len: usize) -> Self {
        assert!((1..=256).contains(&buffer_len), "keyboard buffer must be 1..=256");
        Keyboard { buffer_len, trigger: EdgeTrigger::Rising, data: Rc::new(RefCell::new(KeyboardData::new(buffer_len))) }
    }
    pub fn handle(&self) -> KeyboardHandle {
        Rc::clone(&self.data)
    }
}

impl Component for Keyboard {
    fn name(&self) -> &str {
        "Keyboard"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("buffer_length", self.buffer_len as i64).with("trigger", OptionMap::name_of(self.trigger, &EDGES))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("buffer_length", "Buffer length", 1, 256, 32),
            OptionSpec::choice("trigger", "Clock trigger", &["rising", "falling"], "rising"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["buffer_length", "trigger"])?;
        if let Some(v) = m.int("buffer_length", 1, 256)? {
            self.buffer_len = v as usize;
            let mut d = self.data.borrow_mut();
            d.capacity = self.buffer_len;
            d.buffer.truncate(self.buffer_len);
            d.cursor = d.cursor.min(d.buffer.len());
        }
        self.trigger = m.choice("trigger", &EDGES)?.unwrap_or(self.trigger);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![
            Port::new(20, 10, PortKind::Input, BitWidth::ONE),
            Port::new(0, 0, PortKind::Input, BitWidth::ONE),
            Port::new(10, 10, PortKind::Input, BitWidth::ONE),
            Port::new(130, 10, PortKind::Output, BitWidth::ONE),
            Port::new(140, 10, PortKind::Output, BitWidth::new(7)),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let clear = ctx.port_value(0);
        let clock = ctx.port_value(1);
        let enable = ctx.port_value(2);
        let c = {
            let mut d = self.data.borrow_mut();
            let last = std::mem::replace(&mut d.last_clock, clock);
            if clear == Value::TRUE {
                d.clear();
            } else if enable != Value::FALSE && self.trigger.fired(last, clock) {
                d.dequeue();
            }
            d.char_at(0)
        };
        ctx.set_port(4, Value::known(BitWidth::new(7), c as u64 & 0x7F), DELAY_CHAR);
        ctx.set_port(3, if c != '\0' { Value::TRUE } else { Value::FALSE }, DELAY_AVAILABLE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::testutil::Rig;

    fn rig(len: usize) -> (Rig, KeyboardHandle) {
        let k = Keyboard::new(len);
        let h = k.handle();
        let r = Rig::new(Box::new(k), &[(0, Value::FALSE), (1, Value::FALSE), (2, Value::TRUE)]);
        (r, h)
    }

    fn pulse(r: &mut Rig) {
        r.set(1, Value::TRUE);
        r.set(1, Value::FALSE);
    }

    #[test]
    fn buffer_is_delivered_in_order() {
        let (mut r, h) = rig(8);
        assert_eq!(r.get(3), Value::FALSE);
        for c in "hi".chars() {
            assert!(h.borrow_mut().type_char(c));
        }
        r.refresh();
        let w = BitWidth::new(7);
        assert_eq!((r.get(3), r.get(4)), (Value::TRUE, Value::known(w, b'h' as u64)));
        pulse(&mut r);
        assert_eq!(r.get(4), Value::known(w, b'i' as u64));
        pulse(&mut r);
        assert_eq!((r.get(3), r.get(4)), (Value::FALSE, Value::known(w, 0)));
    }

    #[test]
    fn full_buffer_rejects_and_clear_empties() {
        let (mut r, h) = rig(2);
        assert!(h.borrow_mut().type_char('a') && h.borrow_mut().type_char('b'));
        assert!(!h.borrow_mut().type_char('c'));
        r.refresh();
        r.set(0, Value::TRUE);
        assert!(h.borrow().is_empty());
        assert_eq!(r.get(3), Value::FALSE);
    }

    #[test]
    fn editing_keys() {
        let mut d = KeyboardData::new(8);
        for c in "abc".chars() {
            d.type_char(c);
        }
        assert!(d.move_cursor_by(-2) && d.delete());
        assert_eq!(d.text(), "ac");
        assert!(!d.move_cursor_by(-5));
        d.set_cursor(usize::MAX);
        assert_eq!(d.cursor(), 2);
        assert!(!d.type_char('\u{1}')); // other control characters are ignored
        assert!(d.type_char('\n'));
    }

    #[test]
    fn read_enable_low_holds_the_buffer() {
        let (mut r, h) = rig(4);
        h.borrow_mut().type_char('x');
        r.set(2, Value::FALSE);
        pulse(&mut r);
        assert_eq!(h.borrow().len(), 1);
    }
}
