//! The Plexers library: multiplexer, demultiplexer, decoder, priority encoder, bit selector.
//!
//! Ported from `com.cburch.logisim.std.plexers`. Every part is laid out facing **east**; the circuit
//! rotates it. Port order for each part is documented at the top of its module. Visuals are not ported.

use logisim_core::Value;

use crate::registry::{Library, PartDesc};

pub mod bit_selector;
pub mod decoder;
pub mod demultiplexer;
pub mod multiplexer;
pub mod priority_encoder;

pub use bit_selector::{BitSelector, BitSelectorOptions};
pub use decoder::{Decoder, DecoderOptions};
pub use demultiplexer::{Demultiplexer, DemuxOptions};
pub use multiplexer::{MuxOptions, MuxSize, Multiplexer};
pub use priority_encoder::{PriorityEncoder, PriorityEncoderOptions};

/// Propagation delay of every plexer, in simulator ticks (Logisim's `PlexersLibrary.DELAY`).
pub const DELAY: u32 = 3;

pub(crate) const DISABLED_NAMES: [(&str, DisabledOutput); 2] =
    [("zero", DisabledOutput::Zero), ("floating", DisabledOutput::Floating)];
pub(crate) const SELECT_LOC_NAMES: [(&str, SelectLoc); 2] =
    [("bottom_left", SelectLoc::BottomLeft), ("top_right", SelectLoc::TopRight)];

/// What outputs show while the part is disabled (enable input low).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisabledOutput {
    /// Drive 0 (Logisim default).
    #[default]
    Zero,
    /// Leave floating (high impedance).
    Floating,
}

impl DisabledOutput {
    /// The single-bit value every disabled output bit takes.
    pub(crate) fn bit(self) -> Value {
        match self {
            DisabledOutput::Zero => Value::FALSE,
            DisabledOutput::Floating => Value::UNKNOWN,
        }
    }
}

/// Which side the select input sits on (Logisim's `SELECT_LOC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectLoc {
    /// Bottom/left (Logisim default).
    #[default]
    BottomLeft,
    /// Top/right.
    TopRight,
}

impl SelectLoc {
    /// +1 for bottom-left, -1 for top-right: the sign applied to select-pin offsets.
    pub(crate) fn mult(self) -> i32 {
        match self {
            SelectLoc::BottomLeft => 1,
            SelectLoc::TopRight => -1,
        }
    }
}

/// Outcome of examining the enable input.
pub(crate) enum Enabled {
    /// Enable is low: outputs take the disabled value.
    Off,
    /// Enable is an error value on a connected wire.
    Error,
    /// Part is active.
    On,
}

/// Interpret the enable value as Logisim does (`en == FALSE`, or `ERROR` on a connected pin).
pub(crate) fn classify_enable(en: Value, connected: bool) -> Enabled {
    if en == Value::FALSE {
        Enabled::Off
    } else if en == Value::ERROR && connected {
        Enabled::Error
    } else {
        Enabled::On
    }
}

/// The Plexers library with default options for each part.
pub fn library() -> Library {
    let part = |name, make: fn() -> Box<dyn logisim_core::Component>| PartDesc { name, make };
    Library {
        name: "Plexers",
        parts: vec![
            part("Multiplexer", || Box::new(Multiplexer::new(MuxOptions::default()))),
            part("Demultiplexer", || Box::new(Demultiplexer::new(DemuxOptions::default()))),
            part("Decoder", || Box::new(Decoder::new(DecoderOptions::default()))),
            part("Priority Encoder", || {
                Box::new(PriorityEncoder::new(PriorityEncoderOptions::default()))
            }),
            part("Bit Selector", || Box::new(BitSelector::new(BitSelectorOptions::default()))),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_core::OptionMap;

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
    fn non_default_options_round_trip_and_change_ports() {
        let cases: [(&str, OptionMap); 5] = [
            (
                "Multiplexer",
                OptionMap::new()
                    .with("width", 8)
                    .with("select_bits", 3)
                    .with("enable", true)
                    .with("disabled", "floating")
                    .with("select_loc", "top_right")
                    .with("size", "narrow"),
            ),
            (
                "Demultiplexer",
                OptionMap::new().with("width", 4).with("select_bits", 2).with("enable", true).with("tristate", true),
            ),
            ("Decoder", OptionMap::new().with("select_bits", 3).with("select_loc", "top_right").with("disabled", "floating")),
            ("Priority Encoder", OptionMap::new().with("select_bits", 2).with("disabled", "floating")),
            ("Bit Selector", OptionMap::new().with("width", 16).with("group", 4).with("select_loc", "top_right")),
        ];
        let lib = library();
        for (name, opts) in cases {
            let desc = lib.parts.iter().find(|p| p.name == name).unwrap();
            let default_ports = (desc.make)().ports();
            let mut c = (desc.make)();
            c.set_options(&opts).unwrap();
            let saved = c.options();
            for (k, v) in &opts.0 {
                assert_eq!(saved.0.get(k), Some(v), "{name}.{k}");
            }
            assert_ne!(c.ports(), default_ports, "{name}");
            let mut again = (desc.make)();
            again.set_options(&c.options()).unwrap();
            assert_eq!(again.ports(), c.ports(), "{name}");
        }
    }

    #[test]
    fn bad_options_are_rejected() {
        for p in &library().parts {
            let mut c = (p.make)();
            assert!(c.set_options(&OptionMap::new().with("bogus", 1)).is_err(), "{}", p.name);
            assert!(c.set_options(&OptionMap::new().with("select_bits", 99)).is_err(), "{}", p.name);
            assert!(c.set_options(&OptionMap::new().with("disabled", "maybe")).is_err(), "{}", p.name);
        }
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use crate::wiring::{Pin, PinHandle};
    use logisim_core::{Circuit, CompId, Component, Direction, Location, Value};

    /// A component under test with a [`Pin`] attached to each driven port.
    pub struct Rig {
        pub circuit: Circuit,
        pub id: CompId,
        pins: Vec<(usize, CompId, PinHandle)>,
    }

    impl Rig {
        /// Place `comp` and drive every `(port, value)` in `inputs` from a pin on that port.
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

        /// Change the value driven onto `port` and let the circuit settle.
        pub fn set(&mut self, port: usize, v: Value) {
            let (_, pid, handle) = self
                .pins
                .iter()
                .find(|(p, _, _)| *p == port)
                .expect("port is not driven");
            handle.set(v);
            let pid = *pid;
            self.circuit.refresh(pid);
        }

        pub fn get(&mut self, port: usize) -> Value {
            self.circuit.port_value(self.id, port)
        }
    }
}
