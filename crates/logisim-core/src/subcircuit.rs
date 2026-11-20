//! Subcircuits: a nested [`Circuit`] used as a component. Its interface is a list of ports,
//! each tied to a boundary point inside the nested circuit (Logisim's `SubcircuitFactory`:
//! pins of the inner circuit become the instance's ports).

use crate::circuit::{Circuit, CompId};
use crate::component::{Component, Port, PortKind, PropagateCtx};
use crate::geom::{Direction, Location};
use crate::value::{BitWidth, Value};
use std::cell::Cell;
use std::rc::Rc;

/// One interface port of a subcircuit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubPortSpec {
    pub name: String,
    /// Input ports carry outer values in; output ports carry inner values out.
    pub input: bool,
    pub width: BitWidth,
    /// Where the port touches the nested circuit.
    pub at: Location,
    /// Where the port appears on the instance (east-facing).
    pub offset: Location,
}

/// Inner-side stand-in for an interface port: drives the inner net with the outer value
/// (inputs) and reports the inner net's value (outputs).
struct Boundary {
    width: BitWidth,
    drive: Rc<Cell<Value>>,
    seen: Rc<Cell<Value>>,
}

impl Component for Boundary {
    fn name(&self) -> &str {
        "Boundary"
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::InOut, self.width)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        self.seen.set(ctx.port_value_others(0));
        let d = self.drive.get();
        let d = if d.width() == self.width {
            d
        } else {
            Value::unknown(self.width)
        };
        ctx.set_port(0, d, 1);
    }
}

struct SubPort {
    spec: SubPortSpec,
    drive: Rc<Cell<Value>>,
    seen: Rc<Cell<Value>>,
    id: CompId,
}

pub struct Subcircuit {
    name: String,
    circuit: Circuit,
    ports: Vec<SubPort>,
    started: bool,
}

impl Subcircuit {
    /// Wrap `circuit`; a boundary component is added at each port's `at` point.
    pub fn new(name: &str, mut circuit: Circuit, specs: Vec<SubPortSpec>) -> Self {
        let ports = specs
            .into_iter()
            .map(|spec| {
                let drive = Rc::new(Cell::new(Value::unknown(spec.width)));
                let seen = Rc::new(Cell::new(Value::NIL));
                let b = Boundary {
                    width: spec.width,
                    drive: Rc::clone(&drive),
                    seen: Rc::clone(&seen),
                };
                let id = circuit.add_component(spec.at, Direction::East, Box::new(b));
                SubPort {
                    spec,
                    drive,
                    seen,
                    id,
                }
            })
            .collect();
        Subcircuit {
            name: name.to_string(),
            circuit,
            ports,
            started: false,
        }
    }

    fn drive_outputs(&self, ctx: &mut dyn PropagateCtx) {
        for (i, p) in self.ports.iter().enumerate().filter(|(_, p)| !p.spec.input) {
            let v = if self.circuit.is_oscillating() {
                Value::error(p.spec.width)
            } else {
                p.seen.get()
            };
            let v = if v.width() == p.spec.width {
                v
            } else {
                Value::unknown(p.spec.width)
            };
            ctx.set_port(i, v, 1);
        }
    }
}

impl Component for Subcircuit {
    fn name(&self) -> &str {
        &self.name
    }
    fn ports(&self) -> Vec<Port> {
        self.ports
            .iter()
            .map(|p| {
                let kind = if p.spec.input {
                    PortKind::Input
                } else {
                    PortKind::Output
                };
                Port::new(p.spec.offset.x, p.spec.offset.y, kind, p.spec.width)
            })
            .collect()
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let mut inputs = vec![];
        for (i, p) in self.ports.iter().enumerate().filter(|(_, p)| p.spec.input) {
            p.drive.set(ctx.port_value(i));
            inputs.push(p.id);
        }
        if self.started {
            self.circuit.refresh_many(&inputs);
        } else {
            self.started = true;
            self.circuit.reset();
        }
        self.drive_outputs(ctx);
    }
    fn tick(&mut self, ctx: &mut dyn PropagateCtx) {
        if !self.started {
            return;
        }
        self.circuit.tick();
        self.drive_outputs(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inner circuit: nothing but wire — input boundary wired straight to output boundary.
    #[test]
    fn passthrough_subcircuit_roundtrips_values() {
        struct Src(Value);
        impl Component for Src {
            fn name(&self) -> &str {
                "src"
            }
            fn ports(&self) -> Vec<Port> {
                vec![Port::new(0, 0, PortKind::Output, self.0.width())]
            }
            fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
                ctx.set_port(0, self.0, 1);
            }
        }
        let w4 = BitWidth::new(4);
        let mut inner = Circuit::new();
        inner.add_wire(Location::new(0, 0), Location::new(100, 0));
        let spec = |name: &str, input, at: (i32, i32), off: (i32, i32)| SubPortSpec {
            name: name.into(),
            input,
            width: w4,
            at: Location::new(at.0, at.1),
            offset: Location::new(off.0, off.1),
        };
        let sub = Subcircuit::new(
            "wire",
            inner,
            vec![
                spec("in", true, (0, 0), (-30, 0)),
                spec("out", false, (100, 0), (0, 0)),
            ],
        );
        let mut outer = Circuit::new();
        let id = outer.add_component(Location::new(200, 0), Direction::East, Box::new(sub));
        outer.add_component(
            Location::new(100, 0),
            Direction::East,
            Box::new(Src(Value::known(w4, 0b1011))),
        );
        outer.add_wire(Location::new(100, 0), Location::new(170, 0));
        outer.reset();
        assert_eq!(outer.port_value(id, 1).to_u64(), Some(0b1011));
        assert!(!outer.is_oscillating());
    }
}
