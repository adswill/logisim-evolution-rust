//! Minimal wiring parts: constant sources and input pins.
use logisim_core::{BitWidth, Component, Port, PortKind, PropagateCtx, Value};
use crate::registry::{Library, PartDesc};
use std::cell::Cell;
use std::rc::Rc;

/// The (so far minimal) wiring library.
pub fn library() -> Library {
    Library {
        name: "Wiring",
        parts: vec![
            PartDesc { name: "Pin", make: || Box::new(Pin::bit(false)) },
            PartDesc { name: "Clock", make: || Box::new(Clock { high: false }) },
        ],
    }
}

/// Shared handle for changing a [`Pin`]'s value after it was placed in a circuit.
/// Call `Circuit::refresh(id)` afterwards to propagate the change.
pub type PinHandle = Rc<Cell<Value>>;

/// Drives a settable value on its single output port at (0,0). Also serves as an input pin.
pub struct Pin {
    value: PinHandle,
}

impl Pin {
    pub fn new(value: Value) -> Self {
        Pin {
            value: Rc::new(Cell::new(value)),
        }
    }
    pub fn handle(&self) -> PinHandle {
        Rc::clone(&self.value)
    }
    pub fn bit(high: bool) -> Self {
        Pin::new(if high { Value::TRUE } else { Value::FALSE })
    }
}

impl Component for Pin {
    fn name(&self) -> &str {
        "Pin"
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, self.value.get().width())]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        ctx.set_port(0, self.value.get(), 1);
    }
}

/// A 1-bit clock: toggles on every tick.
pub struct Clock {
    pub high: bool,
}

impl Component for Clock {
    fn name(&self) -> &str {
        "Clock"
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, BitWidth::ONE)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        ctx.set_port(0, if self.high { Value::TRUE } else { Value::FALSE }, 1);
    }
    fn tick(&mut self, ctx: &mut dyn PropagateCtx) {
        self.high = !self.high;
        self.propagate(ctx);
    }
}
