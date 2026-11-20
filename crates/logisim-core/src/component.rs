//! The component abstraction: ports plus a `propagate` function.

use crate::geom::Location;
use crate::option::{OptionKind, OptionMap, OptionSpec};
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
    /// A required input is one the part cannot do anything useful without (a gate's inputs). The
    /// editor warns when a required input is left unconnected; other inputs (enable, clear,
    /// load, ...) may float, as in Logisim. Default: not required.
    pub required: bool,
}

impl Port {
    /// Width of ports that adopt the width of whatever net they touch (probe, pull resistor).
    pub const ANY_WIDTH: BitWidth = BitWidth(0);
    pub fn new(x: i32, y: i32, kind: PortKind, width: BitWidth) -> Self {
        Port {
            offset: Location::new(x, y),
            kind,
            width,
            required: false,
        }
    }
    /// Mark this port as required (see [`Port::required`]).
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// What a component sees and can do while propagating.
pub trait PropagateCtx {
    /// Value currently on the net at port `i` (unknown if nothing drives it).
    fn port_value(&self, i: usize) -> Value;
    /// Like [`port_value`](Self::port_value) but ignoring what *this component* drives on
    /// the net. Needed by bidirectional parts (splitters) to avoid feeding themselves.
    fn port_value_others(&self, i: usize) -> Value;
    /// True if port `i` touches a wire carrying at least one other connection.
    fn is_port_connected(&self, i: usize) -> bool;
    /// Drive port `i` with `v`; takes effect after `delay` ticks (min 1).
    fn set_port(&mut self, i: usize, v: Value, delay: u32);
}

/// A simulated part. Implementations hold their own state (e.g. flip-flop bits).
pub trait Component {
    fn name(&self) -> &str;
    fn ports(&self) -> Vec<Port>;
    /// Tunnels with the same name are joined into one net (port 0 is used).
    fn tunnel_name(&self) -> Option<String> {
        None
    }
    /// Pull resistors: `TRUE`/`FALSE`/`ERROR` pulls floating bits of the net at port 0.
    fn pull_value(&self) -> Option<Value> {
        None
    }
    /// Current options, for saving. Parts without options keep the empty default.
    fn options(&self) -> OptionMap {
        OptionMap::new()
    }
    /// Describes every option for the attribute panel (empty = editor infers widgets from `options()`).
    /// If non-empty it must cover exactly the keys of `options()`; check with [`validate_specs`].
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![]
    }
    /// Apply saved options (called on a freshly made default part, *before* `ports()`).
    /// Absent keys keep the part's default; unknown keys or bad values return `Err`.
    fn set_options(&mut self, opts: &OptionMap) -> Result<(), String> {
        opts.reject_unknown(&[])
    }
    /// Called whenever an input net changes (and once at reset).
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx);
    /// Called on each clock tick (default: ignore).
    fn tick(&mut self, _ctx: &mut dyn PropagateCtx) {}
}

/// Test helper for library authors: checks `option_specs()` against `options()` / `set_options()`.
/// Call it on a freshly made part. Returns a description of the first problem found.
pub fn validate_specs(part: &mut dyn Component) -> Result<(), String> {
    let specs = part.option_specs();
    let opts = part.options();
    if specs.is_empty() {
        return if opts.is_empty() {
            Ok(())
        } else {
            Err(format!("{}: has options but no option_specs", part.name()))
        };
    }
    for s in &specs {
        if !opts.0.contains_key(s.key) {
            return Err(format!(
                "{}: spec '{}' is not an option of the part",
                part.name(),
                s.key
            ));
        }
        let try_set = |part: &mut dyn Component, v: crate::option::OptVal| {
            part.set_options(&OptionMap::new().with(s.key, v))
                .map_err(|e| format!("{}: {e}", part.name()))
        };
        match &s.kind {
            OptionKind::Choice { names } => {
                for n in names {
                    try_set(part, crate::option::OptVal::Str((*n).into()))?;
                }
            }
            OptionKind::Int { min, max } => {
                try_set(part, crate::option::OptVal::Int(*min))?;
                try_set(part, crate::option::OptVal::Int(*max))?;
            }
            OptionKind::Bool | OptionKind::Str => {}
        }
        try_set(part, s.default.clone())?;
    }
    for k in opts.0.keys() {
        if !specs.iter().any(|s| s.key == k) {
            return Err(format!("{}: option '{k}' has no spec", part.name()));
        }
    }
    Ok(())
}
