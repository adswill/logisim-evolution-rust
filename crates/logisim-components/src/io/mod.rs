//! The Input/Output library: Button, DIP Switch, Joystick, Keyboard, LED, LED Bar, RGB LED,
//! 7-Segment Display, Hex Digit Display, LED Matrix and TTY.
//!
//! Ported from `com.cburch.logisim.std.io` (simulation logic and port layout only; painting and
//! mouse/key handling belong to the app). Parts that the user interacts with (button, DIP switch,
//! joystick, keyboard) expose a shared *handle* the UI mutates, then calls `Circuit::refresh(id)`.
//! Parts that display (LEDs, segments, matrix, TTY) expose a handle the UI reads.
//! Port order is documented at the top of each module; all layouts are east-facing.

use logisim_core::Value;

use crate::registry::{Library, PartDesc};

pub mod button;
pub mod dip_switch;
pub mod joystick;
pub mod keyboard;
pub mod led;
pub mod matrix;
pub mod segments;
pub mod tty;

pub use button::{Button, ButtonPress};
pub use dip_switch::{DipReset, DipSwitch, DipSwitchOptions};
pub use joystick::Joystick;
pub use keyboard::{Keyboard, KeyboardData};
pub use led::{Led, RgbLed};
pub use matrix::{LedBar, LedMatrix, MatrixInput, MatrixOptions, MatrixState, SelectLoc};
pub use segments::{HexDigit, SevenSegment, SevenSegmentInput};
pub use tty::{Tty, TtyState};

/// Which clock edge triggers a clocked part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeTrigger {
    #[default]
    Rising,
    Falling,
}

pub(crate) const EDGES: [(&str, EdgeTrigger); 2] = [("rising", EdgeTrigger::Rising), ("falling", EdgeTrigger::Falling)];

impl EdgeTrigger {
    /// True when `last` → `now` is this trigger's active edge.
    pub(crate) fn fired(self, last: Value, now: Value) -> bool {
        match self {
            EdgeTrigger::Rising => last == Value::FALSE && now == Value::TRUE,
            EdgeTrigger::Falling => last == Value::TRUE && now == Value::FALSE,
        }
    }
}

pub fn library() -> Library {
    let part = |name, make: fn() -> Box<dyn logisim_core::Component>| PartDesc { name, make };
    Library {
        name: "Input/Output",
        parts: vec![
            part("Button", || Box::new(Button::new(ButtonPress::Active))),
            part("DIP Switch", || Box::new(DipSwitch::new(DipSwitchOptions::default()))),
            part("Joystick", || Box::new(Joystick::new(4))),
            part("Keyboard", || Box::new(Keyboard::new(32))),
            part("LED", || Box::new(Led::new(true))),
            part("LED Bar", || Box::new(LedBar::new())),
            part("RGB LED", || Box::new(RgbLed::new(true))),
            part("7-Segment Display", || Box::new(SevenSegment::new())),
            part("Hex Digit Display", || Box::new(HexDigit::new())),
            part("LED Matrix", || Box::new(LedMatrix::new(MatrixOptions::default()))),
            part("TTY", || Box::new(Tty::new(8, 32))),
        ],
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use crate::wiring::{Pin, PinHandle};
    use logisim_core::{Circuit, CompId, Component, Direction, Location, Value};

    /// A component under test with a [`Pin`] driving each requested port.
    pub struct Rig {
        pub circuit: Circuit,
        pub id: CompId,
        pins: Vec<(usize, CompId, PinHandle)>,
    }

    impl Rig {
        pub fn new(comp: Box<dyn Component>, inputs: &[(usize, Value)]) -> Rig {
            let mut circuit = Circuit::new();
            let id = circuit.add_component(Location::new(200, 200), Direction::East, comp);
            let mut pins = Vec::new();
            for &(port, v) in inputs {
                let pin = Pin::new(v);
                let handle = pin.handle();
                let at = circuit.port_location(id, port);
                let pid = circuit.add_component(at, Direction::East, Box::new(pin));
                pins.push((port, pid, handle));
            }
            circuit.reset();
            Rig { circuit, id, pins }
        }

        pub fn set(&mut self, port: usize, v: Value) {
            let (_, pid, handle) = self.pins.iter().find(|(p, _, _)| *p == port).expect("port is not driven");
            handle.set(v);
            let pid = *pid;
            self.circuit.refresh(pid);
        }

        /// Re-run the component (after changing a UI handle).
        pub fn refresh(&mut self) {
            self.circuit.refresh(self.id);
        }

        pub fn get(&mut self, port: usize) -> Value {
            self.circuit.port_value(self.id, port)
        }

        /// One clock tick for every component.
        pub fn tick(&mut self) {
            self.circuit.tick();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_core::OptionMap;

    #[test]
    fn option_specs_match_options() {
        for p in &library().parts {
            logisim_core::validate_specs(&mut *(p.make)()).unwrap_or_else(|e| panic!("{}: {e}", p.name));
        }
    }

    #[test]
    fn every_part_round_trips_its_options() {
        for p in &library().parts {
            let part = (p.make)();
            let saved = part.options();
            let mut fresh = (p.make)();
            fresh.set_options(&saved).unwrap_or_else(|e| panic!("{}: {e}", p.name));
            assert_eq!(fresh.options(), saved, "{}", p.name);
            assert_eq!(fresh.ports(), part.ports(), "{}", p.name);
        }
    }

    #[test]
    fn bad_options_are_rejected() {
        for p in &library().parts {
            let mut c = (p.make)();
            assert!(c.set_options(&OptionMap::new().with("bogus", 1)).is_err(), "{}", p.name);
        }
    }
}
