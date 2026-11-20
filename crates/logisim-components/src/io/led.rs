//! LED and RGB LED: indicators that record the value they see for the UI to draw.
//!
//! LED: port 0 = input at (0,0). RGB LED: 0 = red (0,0), 1 = green (-10,10), 2 = blue (-10,-10).
//! `active_high` (default true) lights the LED on a high input; false lights it on a low input.

use std::cell::Cell;
use std::rc::Rc;

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

/// Last value seen by an [`Led`] (initially unknown).
pub type LedHandle = Rc<Cell<Value>>;

pub struct Led {
    active_high: bool,
    seen: LedHandle,
}

impl Led {
    pub fn new(active_high: bool) -> Self {
        Led { active_high, seen: Rc::new(Cell::new(Value::UNKNOWN)) }
    }
    pub fn handle(&self) -> LedHandle {
        Rc::clone(&self.seen)
    }
    /// Whether an LED with this polarity is lit for `v` (Logisim draws only clean 0/1 as lit).
    pub fn is_lit(v: Value, active_high: bool) -> bool {
        v == if active_high { Value::TRUE } else { Value::FALSE }
    }
}

impl Component for Led {
    fn name(&self) -> &str {
        "LED"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("active_high", self.active_high)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::boolean("active_high", "Lit when input is high", true),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["active_high"])?;
        self.active_high = m.boolean("active_high")?.unwrap_or(self.active_high);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Input, BitWidth::ONE)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        self.seen.set(ctx.port_value(0));
    }
}

/// Bit set for each colour input that is high: red = 1, green = 2, blue = 4.
pub type RgbHandle = Rc<Cell<u8>>;

pub struct RgbLed {
    active_high: bool,
    seen: RgbHandle,
}

impl RgbLed {
    pub const RED: u8 = 1;
    pub const GREEN: u8 = 2;
    pub const BLUE: u8 = 4;

    pub fn new(active_high: bool) -> Self {
        RgbLed { active_high, seen: Rc::new(Cell::new(0)) }
    }
    pub fn handle(&self) -> RgbHandle {
        Rc::clone(&self.seen)
    }
    /// Colour channels (same bit layout) that are lit, given the raw "input is high" summary.
    pub fn lit(summary: u8, active_high: bool) -> u8 {
        if active_high {
            summary & 7
        } else {
            !summary & 7
        }
    }
}

impl Component for RgbLed {
    fn name(&self) -> &str {
        "RGB LED"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("active_high", self.active_high)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::boolean("active_high", "Lit when input is high", true),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["active_high"])?;
        self.active_high = m.boolean("active_high")?.unwrap_or(self.active_high);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![
            Port::new(0, 0, PortKind::Input, BitWidth::ONE),
            Port::new(-10, 10, PortKind::Input, BitWidth::ONE),
            Port::new(-10, -10, PortKind::Input, BitWidth::ONE),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let mut summary = 0;
        for i in 0..3 {
            if ctx.port_value(i) == Value::TRUE {
                summary |= 1 << i;
            }
        }
        self.seen.set(summary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::testutil::Rig;

    #[test]
    fn led_records_input() {
        let led = Led::new(true);
        let h = led.handle();
        let mut r = Rig::new(Box::new(led), &[(0, Value::FALSE)]);
        assert!(!Led::is_lit(h.get(), true));
        r.set(0, Value::TRUE);
        assert_eq!(h.get(), Value::TRUE);
        assert!(Led::is_lit(h.get(), true));
        assert!(!Led::is_lit(h.get(), false));
        r.set(0, Value::UNKNOWN);
        assert!(!Led::is_lit(h.get(), true) && !Led::is_lit(h.get(), false));
    }

    #[test]
    fn rgb_channels() {
        let led = RgbLed::new(true);
        let h = led.handle();
        let mut r = Rig::new(Box::new(led), &[(0, Value::TRUE), (1, Value::FALSE), (2, Value::TRUE)]);
        assert_eq!(h.get(), RgbLed::RED | RgbLed::BLUE);
        assert_eq!(RgbLed::lit(h.get(), false), RgbLed::GREEN);
        r.set(1, Value::TRUE);
        assert_eq!(h.get(), 7);
    }
}
