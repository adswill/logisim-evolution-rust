//! Signal values and the simulation engine.
pub mod circuit;
pub mod component;
pub mod geom;
pub mod option;
pub mod subcircuit;
pub mod value;

pub use circuit::{Circuit, CompId, Wire};
pub use component::{Component, Port, PortKind, PropagateCtx};
pub use geom::{Direction, Location};
pub use option::{OptVal, OptionMap};
pub use subcircuit::{SubPortSpec, Subcircuit};
pub use value::{BitWidth, Value};
