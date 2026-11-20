//! The component abstraction: ports plus a `propagate` function.

use crate::geom::Location;
use crate::value::{BitWidth, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    Input,
    Output,
    InOut,
}

/// A connection point, at an offset from the component's origin (east-facing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Port {
    pub offset: Location,
    pub kind: PortKind,
    pub width: BitWidth,
}

impl Port {
    pub fn new(x: i32, y: i32, kind: PortKind, width: BitWidth) -> Self {
        Port { offset: Location::new(x, y), kind, width }
    }
}

/// What a component sees and can do while propagating.
pub trait PropagateCtx {
    /// Value currently on the net at port `i` (unknown if nothing drives it).
    fn port_value(&self, i: usize) -> Value;
    /// True if port `i` touches a wire carrying at least one other connection.
    fn is_port_connected(&self, i: usize) -> bool;
    /// Drive port `i` with `v`; takes effect after `delay` ticks (min 1).
    fn set_port(&mut self, i: usize, v: Value, delay: u32);
}

/// A simulated part. Implementations hold their own state (e.g. flip-flop bits).
pub trait Component {
    fn name(&self) -> &str;
    fn ports(&self) -> Vec<Port>;
    /// Called whenever an input net changes (and once at reset).
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx);
    /// Called on each clock tick (default: ignore).
    fn tick(&mut self, _ctx: &mut dyn PropagateCtx) {}
}
