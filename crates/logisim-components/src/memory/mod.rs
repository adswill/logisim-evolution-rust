//! The Memory library: flip-flops, register, counter, shift register, random generator, RAM, ROM.
//!
//! Ported from `com.cburch.logisim.std.memory`. Port positions follow Logisim's *classic*
//! appearance, translated so that port 0 (the primary output) sits at (0,0), as the component
//! contract requires. Each module documents its own port order.
//!
//! Not ported (UI / out of scope): painting, pokers, hex editor, HDL generators, the
//! "Evolution" appearance's alternative port positions.

use logisim_core::{Component, Value};

use crate::registry::{Library, PartDesc};

pub mod contents;
pub mod counter;
pub mod flipflop;
pub mod mem;
pub mod random;
pub mod ram;
pub mod register;
pub mod rom;
pub mod shift_register;

pub use contents::MemContents;
pub use counter::{Counter, CounterOptions, OnGoal};
pub use flipflop::{FlipFlop, FlipFlopKind, FlipFlopOptions};
pub use mem::{DataBus, EnableMode, LineSize, ReadBehavior};
pub use ram::{Ram, RamOptions, RamType};
pub use random::{Random, RandomOptions};
pub use register::{Register, RegisterOptions};
pub use rom::{Rom, RomOptions};
pub use shift_register::{ShiftRegister, ShiftRegisterOptions};

/// The Memory library with default options for each part.
pub fn library() -> Library {
    let part = |name, make: fn() -> Box<dyn Component>| PartDesc { name, make };
    Library {
        name: "Memory",
        parts: vec![
            part("D Flip-Flop", || Box::new(FlipFlop::new(FlipFlopKind::D))),
            part("T Flip-Flop", || Box::new(FlipFlop::new(FlipFlopKind::T))),
            part("J-K Flip-Flop", || Box::new(FlipFlop::new(FlipFlopKind::Jk))),
            part("S-R Flip-Flop", || Box::new(FlipFlop::new(FlipFlopKind::Sr))),
            part("Register", || Box::new(Register::new(RegisterOptions::default()))),
            part("Counter", || Box::new(Counter::new(CounterOptions::default()))),
            part("Shift Register", || {
                Box::new(ShiftRegister::new(ShiftRegisterOptions::default()))
            }),
            part("Random", || Box::new(Random::new(RandomOptions::default()))),
            part("RAM", || Box::new(Ram::new(RamOptions::default()))),
            part("ROM", || Box::new(Rom::new(RomOptions::default()))),
        ],
    }
}

/// Flip-flop propagation delay (ticks).
pub const FLIPFLOP_DELAY: u32 = 5;
/// Register and counter delay.
pub const REGISTER_DELAY: u32 = 8;
/// Shift register and random generator delay.
pub const SHIFT_DELAY: u32 = 4;
/// RAM / ROM delay.
pub const MEM_DELAY: u32 = 10;

/// What makes a clocked part act (Logisim's `StdAttr.TRIGGER`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Trigger {
    #[default]
    Rising,
    Falling,
    /// Level-sensitive: active while the clock is 1.
    High,
    /// Level-sensitive: active while the clock is 0.
    Low,
}

impl Trigger {
    /// True for the edge triggers (the only kinds some parts accept).
    pub fn is_edge(self) -> bool {
        matches!(self, Trigger::Rising | Trigger::Falling)
    }
}

/// Remembers the previous clock value and detects the trigger condition (Java `ClockState`).
#[derive(Debug, Clone, Copy)]
pub struct ClockState {
    last: Value,
}

impl Default for ClockState {
    fn default() -> Self {
        ClockState { last: Value::FALSE }
    }
}

impl ClockState {
    /// Records `new` and reports whether the trigger fired.
    pub fn update(&mut self, new: Value, trigger: Trigger) -> bool {
        let old = std::mem::replace(&mut self.last, new);
        match trigger {
            Trigger::Rising => old == Value::FALSE && new == Value::TRUE,
            Trigger::Falling => old == Value::TRUE && new == Value::FALSE,
            Trigger::High => new == Value::TRUE,
            Trigger::Low => new == Value::FALSE,
        }
    }
}

/// Power-on contents of state-holding parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Startup {
    /// All zeros (Logisim default).
    #[default]
    Zero,
    /// Unknown (`x`) until first written.
    Unknown,
}

/// Java `Value.toLongValue()`: all-ones for anything not fully defined.
pub(crate) fn long_value(v: Value) -> u64 {
    v.to_u64().unwrap_or(u64::MAX)
}

#[cfg(test)]
pub(crate) mod testutil {
    //! A tiny test bench: one part under test, with input pins wired to any of its ports.
    use logisim_core::{Circuit, CompId, Component, Direction, Location, Value};

    use crate::wiring::{Pin, PinHandle};

    pub struct Bench {
        pub c: Circuit,
        pub id: CompId,
        origin: Location,
        ports: Vec<Location>,
        segs: Vec<(Location, Location)>,
        pins: Vec<(usize, PinHandle, CompId)>,
    }

    fn on_seg(s: (Location, Location), p: Location) -> bool {
        p.x >= s.0.x.min(s.1.x)
            && p.x <= s.0.x.max(s.1.x)
            && p.y >= s.0.y.min(s.1.y)
            && p.y <= s.0.y.max(s.1.y)
    }

    impl Bench {
        pub fn new(comp: Box<dyn Component>) -> Bench {
            let mut c = Circuit::new();
            let origin = Location::new(1000, 1000);
            let n = comp.ports().len();
            let id = c.add_component(origin, Direction::East, comp);
            let ports = (0..n).map(|i| c.port_location(id, i)).collect();
            Bench {
                c,
                id,
                origin,
                ports,
                segs: vec![],
                pins: vec![],
            }
        }

        pub fn port_loc(&self, i: usize) -> Location {
            self.ports[i]
        }
        pub fn origin(&self) -> Location {
            self.origin
        }

        /// Attach a pin with initial value `v` to port `port`, routed so that it never touches
        /// another port or wire.
        pub fn drive(&mut self, port: usize, v: Value) {
            let at = self.ports[port];
            let k = self.pins.len() as i32;
            let dist = 100 + 20 * k;
            for (dx, dy) in [(-1, 0), (0, 1), (0, -1)] {
                let end = Location::new(at.x + dx * dist, at.y + dy * dist);
                let seg = (at, end);
                let hits_port = self
                    .ports
                    .iter()
                    .enumerate()
                    .any(|(i, p)| i != port && on_seg(seg, *p));
                let hits_wire = self.segs.iter().any(|s| {
                    on_seg(*s, end) || on_seg(*s, at) || on_seg(seg, s.0) || on_seg(seg, s.1)
                });
                if hits_port || hits_wire {
                    continue;
                }
                let pin = Pin::new(v);
                let handle = pin.handle();
                let pid = self.c.add_component(end, Direction::East, Box::new(pin));
                self.c.add_wire(end, at);
                self.segs.push(seg);
                self.pins.push((port, handle, pid));
                return;
            }
            panic!("no free route for port {port}");
        }

        pub fn start(&mut self) {
            self.c.reset();
        }

        /// Change the pin driving `port` and let the circuit settle.
        pub fn set(&mut self, port: usize, v: Value) {
            let (_, h, pid) = self
                .pins
                .iter()
                .find(|(p, _, _)| *p == port)
                .expect("port not driven")
                .clone();
            h.set(v);
            self.c.refresh(pid);
        }

        /// Pulse a 1-bit pin 0→1→0.
        pub fn pulse(&mut self, port: usize) {
            self.set(port, Value::TRUE);
            self.set(port, Value::FALSE);
        }

        pub fn out(&mut self, port: usize) -> Value {
            self.c.port_value(self.id, port)
        }
    }
}
