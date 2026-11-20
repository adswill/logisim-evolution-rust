//! Joystick: two outputs (x and y, `bits` wide) following a stick position of -14..=14 on each axis.
//!
//! Port order: 0 = x at (0,0); 1 = y at (0,10). The UI writes the position through the handle.

use std::cell::Cell;
use std::rc::Rc;

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

/// Stick position `(x, y)`, each -14..=14 (0 = centered).
pub type JoystickHandle = Rc<Cell<(i32, i32)>>;

pub struct Joystick {
    bits: u8,
    pos: JoystickHandle,
}

impl Joystick {
    /// `bits` is 2..=5.
    pub fn new(bits: u8) -> Self {
        assert!((2..=5).contains(&bits), "joystick bits must be 2..=5");
        Joystick { bits, pos: Rc::new(Cell::new((0, 0))) }
    }
    pub fn handle(&self) -> JoystickHandle {
        Rc::clone(&self.pos)
    }

    /// Map a stick coordinate to the output code (Logisim's formula).
    fn code(&self, p: i32) -> u64 {
        let steps = (1i32 << self.bits) - 1;
        let mut v = (p.clamp(-14, 14) + 14) * steps / 29 + 1;
        if self.bits > 4 && v >= steps / 2 {
            v += 1;
        }
        v as u64
    }
}

impl Component for Joystick {
    fn name(&self) -> &str {
        "Joystick"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("bits", self.bits)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("bits", "Bit width per axis", 2, 5, 4),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["bits"])?;
        if let Some(v) = m.int("bits", 2, 5)? {
            self.bits = v as u8;
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let w = BitWidth::new(self.bits);
        vec![Port::new(0, 0, PortKind::Output, w), Port::new(0, 10, PortKind::Output, w)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let (x, y) = self.pos.get();
        let w = BitWidth::new(self.bits);
        ctx.set_port(0, Value::known(w, self.code(x)), 1);
        ctx.set_port(1, Value::known(w, self.code(y)), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::testutil::Rig;

    #[test]
    fn extremes_and_center_4_bits() {
        let j = Joystick::new(4);
        let h = j.handle();
        let mut r = Rig::new(Box::new(j), &[]);
        let w = BitWidth::new(4);
        // centered: (0+14)*15/29+1 = 8
        assert_eq!((r.get(0), r.get(1)), (Value::known(w, 8), Value::known(w, 8)));
        h.set((-14, 14));
        r.refresh();
        assert_eq!((r.get(0), r.get(1)), (Value::known(w, 1), Value::known(w, 15)));
    }

    #[test]
    fn five_bit_skips_the_middle_code() {
        let j = Joystick::new(5);
        // steps=31; code for p=0 is (14*31/29)+1 = 15 >= 15 -> 16
        assert_eq!(j.code(0), 16);
        assert_eq!(j.code(-14), 1);
        assert_eq!(j.code(14), 31);
    }
}
