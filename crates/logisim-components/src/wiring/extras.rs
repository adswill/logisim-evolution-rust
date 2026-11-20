//! Wiring leftovers: Do Not Connect and Power On Reset.

use std::cell::Cell;
use std::rc::Rc;

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

/// Marks a point that is deliberately left unconnected: a bidirectional pin that never drives.
///
/// Port order: 0 = the pin at (0,0). Ported from Logisim's `DoNotConnect` (its `propagate` does nothing).
pub struct DoNotConnect {
    width: BitWidth,
}

impl DoNotConnect {
    pub fn new(width: u8) -> Self {
        DoNotConnect { width: BitWidth::new(width) }
    }
}

impl Component for DoNotConnect {
    fn name(&self) -> &str {
        "Do Not Connect"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width.get())
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![OptionSpec::int("width", "Data bits", 1, 64, 1)]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width"])?;
        if let Some(v) = m.int("width", 1, 64)? {
            self.width = BitWidth::new(v as u8);
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::InOut, self.width)]
    }
    fn propagate(&mut self, _ctx: &mut dyn PropagateCtx) {}
}

/// Direction of the reset pulse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PorTransition {
    /// Starts high, drops low when the reset ends (Logisim default).
    #[default]
    HighToLow,
    /// Starts low, rises high when the reset ends.
    LowToHigh,
}

const TRANSITIONS: [(&str, PorTransition); 2] = [("high_to_low", PorTransition::HighToLow), ("low_to_high", PorTransition::LowToHigh)];

/// Ticks the part has been running; the UI sets it to 0 (and refreshes) to re-trigger the reset.
pub type PorHandle = Rc<Cell<u32>>;

/// Power-on reset: the output holds its start level for `duration` simulation ticks, then flips.
///
/// Port order: 0 = output at (0,0). Logisim times this with a wall-clock timer (`duration` seconds);
/// here the duration counts simulator ticks so runs are reproducible.
pub struct PowerOnReset {
    transition: PorTransition,
    duration: u32,
    elapsed: PorHandle,
}

impl PowerOnReset {
    /// `duration` is 1..=10 ticks (Logisim default 2).
    pub fn new(transition: PorTransition, duration: u32) -> Self {
        assert!((1..=10).contains(&duration), "duration must be 1..=10");
        PowerOnReset { transition, duration, elapsed: Rc::new(Cell::new(0)) }
    }
    pub fn handle(&self) -> PorHandle {
        Rc::clone(&self.elapsed)
    }
    fn level(&self) -> Value {
        let done = self.elapsed.get() >= self.duration;
        let high = (self.transition == PorTransition::HighToLow) != done;
        if high {
            Value::TRUE
        } else {
            Value::FALSE
        }
    }
}

impl Component for PowerOnReset {
    fn name(&self) -> &str {
        "Power On Reset"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("transition", OptionMap::name_of(self.transition, &TRANSITIONS)).with("duration", self.duration)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::choice("transition", "Reset pulse", &["high_to_low", "low_to_high"], "high_to_low"),
            OptionSpec::int("duration", "Reset duration (ticks)", 1, 10, 2),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["transition", "duration"])?;
        self.transition = m.choice("transition", &TRANSITIONS)?.unwrap_or(self.transition);
        if let Some(v) = m.int("duration", 1, 10)? {
            self.duration = v as u32;
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, BitWidth::ONE)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        ctx.set_port(0, self.level(), 1);
    }
    fn tick(&mut self, ctx: &mut dyn PropagateCtx) {
        if self.elapsed.get() < self.duration {
            self.elapsed.set(self.elapsed.get() + 1);
        }
        self.propagate(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_core::{Circuit, Direction, Location};

    fn setup(c: Box<dyn Component>) -> (Circuit, logisim_core::CompId) {
        let mut circ = Circuit::new();
        let id = circ.add_component(Location::new(100, 100), Direction::East, c);
        circ.reset();
        (circ, id)
    }

    #[test]
    fn reset_pulse_ends_after_duration_ticks() {
        for (tr, before, after) in [(PorTransition::HighToLow, Value::TRUE, Value::FALSE), (PorTransition::LowToHigh, Value::FALSE, Value::TRUE)] {
            let por = PowerOnReset::new(tr, 2);
            let handle = por.handle();
            let (mut c, id) = setup(Box::new(por));
            assert_eq!(c.port_value(id, 0), before);
            c.tick();
            assert_eq!(c.port_value(id, 0), before);
            c.tick();
            assert_eq!(c.port_value(id, 0), after);
            handle.set(0); // poke: restart the reset
            c.refresh(id);
            assert_eq!(c.port_value(id, 0), before);
        }
    }

    #[test]
    fn do_not_connect_never_drives() {
        let (mut c, id) = setup(Box::new(DoNotConnect::new(4)));
        assert_eq!(c.port_value(id, 0), Value::unknown(BitWidth::new(4)));
    }
}
