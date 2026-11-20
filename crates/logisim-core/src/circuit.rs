//! Circuits: placed components + wires, resolved into nets, simulated event by event.

use crate::component::{Component, PortKind, PropagateCtx};
use crate::geom::{Direction, Location};
use crate::value::{BitWidth, Value};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// Index of a component in a [`Circuit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CompId(pub usize);

struct Placed {
    loc: Location,
    facing: Direction,
    mirror: bool,
    comp: Box<dyn Component>,
    ports: Vec<crate::component::Port>,
}

impl Placed {
    fn port_loc(&self, i: usize) -> Location {
        let o = self.facing.place(self.mirror, self.ports[i].offset);
        self.loc.translate(o.x, o.y)
    }
}

/// A wire segment (axis-aligned) between two grid points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wire {
    pub a: Location,
    pub b: Location,
}

impl Wire {
    fn contains(&self, p: Location) -> bool {
        let (x0, x1) = (self.a.x.min(self.b.x), self.a.x.max(self.b.x));
        let (y0, y1) = (self.a.y.min(self.b.y), self.a.y.max(self.b.y));
        p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1
    }
}

#[derive(Default)]
struct Nets {
    /// net index for every grid point of interest
    of_point: HashMap<Location, usize>,
    count: usize,
    /// per net: connection count (ports + wires)
    members: Vec<usize>,
    /// per net: width, or None if no ports on it
    width: Vec<Option<BitWidth>>,
    /// per net: pull-up/down/error requested by pull resistors
    pull: Vec<Option<Value>>,
    /// nets whose ports disagree on width
    width_errors: Vec<usize>,
}

#[derive(PartialEq, Eq)]
struct Event {
    time: u64,
    serial: u64,
    comp: CompId,
    port: usize,
    value: Value,
}
impl Ord for Event {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.time, self.serial).cmp(&(o.time, o.serial))
    }
}
impl PartialOrd for Event {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

/// Default number of propagation steps before a circuit is declared oscillating.
pub const DEFAULT_SIM_LIMIT: u32 = 1000;

pub struct Circuit {
    placed: Vec<Placed>,
    wires: Vec<Wire>,
    nets: Option<Nets>,
    /// last value driven by each (component, port)
    drivers: HashMap<(CompId, usize), Value>,
    net_values: Vec<Value>,
    queue: BinaryHeap<Reverse<Event>>,
    clock: u64,
    serial: u64,
    oscillating: bool,
    pub sim_limit: u32,
}

impl Default for Circuit {
    fn default() -> Self {
        Self::new()
    }
}

impl Circuit {
    pub fn new() -> Self {
        Circuit {
            placed: vec![],
            wires: vec![],
            nets: None,
            drivers: HashMap::new(),
            net_values: vec![],
            queue: BinaryHeap::new(),
            clock: 0,
            serial: 0,
            oscillating: false,
            sim_limit: DEFAULT_SIM_LIMIT,
        }
    }

    pub fn add_component(
        &mut self,
        loc: Location,
        facing: Direction,
        comp: Box<dyn Component>,
    ) -> CompId {
        self.add_component_oriented(loc, facing, false, comp)
    }

    /// Like [`add_component`](Self::add_component), optionally mirrored top-to-bottom
    /// (see [`Direction::place`]).
    pub fn add_component_oriented(
        &mut self,
        loc: Location,
        facing: Direction,
        mirror: bool,
        comp: Box<dyn Component>,
    ) -> CompId {
        let ports = comp.ports();
        self.placed.push(Placed {
            loc,
            facing,
            mirror,
            comp,
            ports,
        });
        self.nets = None;
        CompId(self.placed.len() - 1)
    }
    pub fn add_wire(&mut self, a: Location, b: Location) {
        assert!(
            a.x == b.x || a.y == b.y,
            "wires must be horizontal or vertical"
        );
        self.wires.push(Wire { a, b });
        self.nets = None;
    }
    pub fn component_mut(&mut self, id: CompId) -> &mut dyn Component {
        self.nets = None; // ports may change
        &mut *self.placed[id.0].comp
    }
    pub fn port_location(&self, id: CompId, port: usize) -> Location {
        self.placed[id.0].port_loc(port)
    }
    pub fn is_oscillating(&self) -> bool {
        self.oscillating
    }
    pub fn clock(&self) -> u64 {
        self.clock
    }

    // ---- net construction ----------------------------------------------------

    fn build_nets(&mut self) {
        // points of interest: wire endpoints and port locations
        let mut pts: Vec<Location> = vec![];
        for w in &self.wires {
            pts.push(w.a);
            pts.push(w.b);
        }
        for p in &self.placed {
            for i in 0..p.ports.len() {
                pts.push(p.port_loc(i));
            }
        }
        pts.sort();
        pts.dedup();
        let idx: HashMap<Location, usize> = pts.iter().enumerate().map(|(i, p)| (*p, i)).collect();
        let mut uf: Vec<usize> = (0..pts.len()).collect();
        fn find(uf: &mut [usize], mut x: usize) -> usize {
            while uf[x] != x {
                uf[x] = uf[uf[x]];
                x = uf[x];
            }
            x
        }
        // each wire joins every point of interest lying on it (also T-junctions)
        for w in &self.wires {
            let first = idx[&w.a];
            for (p, &i) in &idx {
                if w.contains(*p) {
                    let (ra, rb) = (find(&mut uf, first), find(&mut uf, i));
                    uf[ra] = rb;
                }
            }
        }
        // tunnels with equal names are one net
        let mut by_name: HashMap<String, usize> = HashMap::new();
        for p in &self.placed {
            if let Some(name) = p.comp.tunnel_name() {
                let i = idx[&p.port_loc(0)];
                match by_name.get(&name) {
                    Some(&j) => {
                        let (a, b) = (find(&mut uf, i), find(&mut uf, j));
                        uf[a] = b;
                    }
                    None => {
                        by_name.insert(name, i);
                    }
                }
            }
        }
        let mut roots: HashMap<usize, usize> = HashMap::new();
        let mut nets = Nets::default();
        for (i, p) in pts.iter().enumerate() {
            let r = find(&mut uf, i);
            let n = *roots.entry(r).or_insert_with(|| {
                nets.count += 1;
                nets.count - 1
            });
            nets.of_point.insert(*p, n);
        }
        nets.members = vec![0; nets.count];
        nets.width = vec![None; nets.count];
        nets.pull = vec![None; nets.count];
        let mut pulls: Vec<(bool, bool, bool)> = vec![(false, false, false); nets.count];
        let mut bad = HashSet::new();
        for p in &self.placed {
            if let Some(pv) = p.comp.pull_value() {
                let e = &mut pulls[nets.of_point[&p.port_loc(0)]];
                e.0 |= pv == Value::TRUE;
                e.1 |= pv == Value::FALSE;
                e.2 |= pv == Value::ERROR;
            }
            for (i, port) in p.ports.iter().enumerate() {
                let n = nets.of_point[&p.port_loc(i)];
                nets.members[n] += 1;
                if port.width == crate::component::Port::ANY_WIDTH {
                    continue;
                }
                match nets.width[n] {
                    None => nets.width[n] = Some(port.width),
                    Some(w) if w != port.width => {
                        bad.insert(n);
                    }
                    _ => {}
                }
            }
        }
        for w in &self.wires {
            nets.members[nets.of_point[&w.a]] += 1;
        }
        for (n, (up, down, err)) in pulls.into_iter().enumerate() {
            nets.pull[n] = if err {
                Some(Value::ERROR)
            } else if up && !down {
                Some(Value::TRUE)
            } else if down && !up {
                Some(Value::FALSE)
            } else {
                None // none, or up and down cancel out
            };
        }
        nets.width_errors = bad.into_iter().collect();
        nets.width_errors.sort();
        self.nets = Some(nets);
        self.recompute_all_nets();
    }

    fn ensure_nets(&mut self) {
        if self.nets.is_none() {
            self.build_nets();
        }
    }

    /// Nets with mismatched port widths (for UI highlighting).
    pub fn width_error_nets(&mut self) -> usize {
        self.ensure_nets();
        self.nets.as_ref().unwrap().width_errors.len()
    }

    fn net_of(&self, id: CompId, port: usize) -> usize {
        self.nets.as_ref().unwrap().of_point[&self.placed[id.0].port_loc(port)]
    }

    fn net_value(&self, n: usize) -> Value {
        self.net_value_excluding(n, None)
    }

    /// Combined value of a net: all drivers (minus `exclude`), then pull resistors.
    fn net_value_excluding(&self, n: usize, exclude: Option<CompId>) -> Value {
        let nets = self.nets.as_ref().unwrap();
        let Some(w) = nets.width[n] else {
            return Value::NIL;
        };
        let mut v = Value::NIL;
        for ((id, port), val) in &self.drivers {
            if Some(*id) != exclude && self.net_of(*id, *port) == n {
                v = v.combine(*val);
            }
        }
        if v == Value::NIL {
            v = Value::unknown(w);
        }
        match nets.pull[n] {
            Some(p) => {
                let bits: Vec<Value> = v
                    .bits()
                    .into_iter()
                    .map(|b| if b == Value::UNKNOWN { p } else { b })
                    .collect();
                Value::from_bits(&bits)
            }
            None => v,
        }
    }

    fn recompute_all_nets(&mut self) {
        let count = self.nets.as_ref().unwrap().count;
        self.net_values = (0..count).map(|n| self.net_value(n)).collect();
    }

    /// Value on the net at a grid location (after [`settle`](Self::settle)).
    pub fn value_at(&mut self, loc: Location) -> Value {
        self.ensure_nets();
        let nets = self.nets.as_ref().unwrap();
        nets.of_point
            .get(&loc)
            .map_or(Value::NIL, |&n| self.net_values[n])
    }
    pub fn port_value(&mut self, id: CompId, port: usize) -> Value {
        let l = self.port_location(id, port);
        self.value_at(l)
    }

    // ---- simulation ------------------------------------------------------------

    fn schedule(&mut self, comp: CompId, port: usize, value: Value, delay: u32) {
        self.serial += 1;
        let time = self.clock + delay.max(1) as u64;
        self.queue.push(Reverse(Event {
            time,
            serial: self.serial,
            comp,
            port,
            value,
        }));
    }

    fn run_component(&mut self, id: CompId, tick: bool) {
        let mut ctx = Ctx {
            circuit: self,
            id,
            out: vec![],
        };
        // temporarily take the component out so the context can borrow the circuit
        let mut comp = std::mem::replace(&mut ctx.circuit.placed[id.0].comp, Box::new(Dummy));
        if tick {
            comp.tick(&mut ctx);
        } else {
            comp.propagate(&mut ctx);
        }
        let out = std::mem::take(&mut ctx.out);
        self.placed[id.0].comp = comp;
        for (port, v, d) in out {
            self.schedule(id, port, v, d);
        }
    }

    /// Re-run every component (like a simulator reset), then settle.
    pub fn reset(&mut self) {
        self.ensure_nets();
        self.drivers.clear();
        self.queue.clear();
        self.oscillating = false;
        self.recompute_all_nets();
        for i in 0..self.placed.len() {
            self.run_component(CompId(i), false);
        }
        self.settle();
    }

    /// Process events until the circuit is stable or the step limit is hit.
    pub fn settle(&mut self) {
        self.ensure_nets();
        self.oscillating = false;
        let mut steps = 0u32;
        while let Some(Reverse(first)) = self.queue.pop() {
            steps += 1;
            if steps > self.sim_limit {
                self.oscillating = true;
                self.queue.clear();
                return;
            }
            self.clock = first.time;
            let mut batch = vec![first];
            while self
                .queue
                .peek()
                .is_some_and(|Reverse(e)| e.time == self.clock)
            {
                batch.push(self.queue.pop().unwrap().0);
            }
            let mut dirty: HashSet<usize> = HashSet::new();
            for e in batch {
                let n = self.net_of(e.comp, e.port);
                self.drivers.insert((e.comp, e.port), e.value);
                dirty.insert(n);
            }
            let mut changed = vec![];
            for n in dirty {
                let v = self.net_value(n);
                if v != self.net_values[n] {
                    self.net_values[n] = v;
                    changed.push(n);
                }
            }
            // wake every component with an input-ish port on a changed net
            let mut wake: Vec<CompId> = vec![];
            for (ci, p) in self.placed.iter().enumerate() {
                let hit = p.ports.iter().enumerate().any(|(i, port)| {
                    port.kind != PortKind::Output && changed.contains(&self.net_of(CompId(ci), i))
                });
                if hit {
                    wake.push(CompId(ci));
                }
            }
            for id in wake {
                self.run_component(id, false);
            }
        }
    }

    /// Re-evaluate one component (e.g. after its pin value was changed) and settle.
    pub fn refresh(&mut self, id: CompId) {
        self.ensure_nets();
        self.run_component(id, false);
        self.settle();
    }

    /// Re-evaluate several components, then settle once.
    pub fn refresh_many(&mut self, ids: &[CompId]) {
        self.ensure_nets();
        for &id in ids {
            self.run_component(id, false);
        }
        self.settle();
    }

    /// Tick all clocked components once and settle.
    pub fn tick(&mut self) {
        self.ensure_nets();
        for i in 0..self.placed.len() {
            self.run_component(CompId(i), true);
        }
        self.settle();
    }
}

struct Dummy;
impl Component for Dummy {
    fn name(&self) -> &str {
        "dummy"
    }
    fn ports(&self) -> Vec<crate::component::Port> {
        vec![]
    }
    fn propagate(&mut self, _: &mut dyn PropagateCtx) {}
}

struct Ctx<'a> {
    circuit: &'a mut Circuit,
    id: CompId,
    out: Vec<(usize, Value, u32)>,
}

impl PropagateCtx for Ctx<'_> {
    fn port_value(&self, i: usize) -> Value {
        let n = self.circuit.net_of(self.id, i);
        self.circuit.net_values[n]
    }
    fn port_value_others(&self, i: usize) -> Value {
        let n = self.circuit.net_of(self.id, i);
        self.circuit.net_value_excluding(n, Some(self.id))
    }
    fn is_port_connected(&self, i: usize) -> bool {
        let c = &*self.circuit;
        let nets = c.nets.as_ref().unwrap();
        // the net holds this port itself; connected if anything else shares it
        nets.members[c.net_of(self.id, i)] > 1
    }
    fn set_port(&mut self, i: usize, v: Value, delay: u32) {
        self.out.push((i, v, delay));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Port;

    /// Test source: drives a fixed value on port 0.
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
    /// Test inverter, west input at (-10,0).
    struct Inv;
    impl Component for Inv {
        fn name(&self) -> &str {
            "inv"
        }
        fn ports(&self) -> Vec<Port> {
            vec![
                Port::new(0, 0, PortKind::Output, BitWidth::ONE),
                Port::new(-10, 0, PortKind::Input, BitWidth::ONE),
            ]
        }
        fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
            let v = !ctx.port_value(1);
            ctx.set_port(0, v, 1);
        }
    }

    #[test]
    fn wire_carries_value_through_inverter() {
        let mut c = Circuit::new();
        c.add_component(
            Location::new(0, 0),
            Direction::East,
            Box::new(Src(Value::TRUE)),
        );
        let inv = c.add_component(Location::new(30, 0), Direction::East, Box::new(Inv));
        c.add_wire(Location::new(0, 0), Location::new(20, 0));
        c.reset();
        assert_eq!(c.port_value(inv, 0), Value::FALSE);
        assert!(!c.is_oscillating());
    }

    #[test]
    fn mirrored_part_has_its_ports_flipped_top_to_bottom() {
        let mut c = Circuit::new();
        let plain = c.add_component(Location::new(0, 0), Direction::East, Box::new(Inv));
        let flipped =
            c.add_component_oriented(Location::new(100, 0), Direction::East, true, Box::new(Inv));
        // Inv ports: out (0,0), in (-10,0): on the mirror axis, so unchanged apart from position
        assert_eq!(c.port_location(plain, 1), Location::new(-10, 0));
        assert_eq!(c.port_location(flipped, 1), Location::new(90, 0));
        // an off-axis part: AND-like ports at y = +-20
        struct Two;
        impl Component for Two {
            fn name(&self) -> &str {
                "two"
            }
            fn ports(&self) -> Vec<Port> {
                vec![
                    Port::new(0, 0, PortKind::Output, BitWidth::ONE),
                    Port::new(-40, -20, PortKind::Input, BitWidth::ONE),
                    Port::new(-40, 20, PortKind::Input, BitWidth::ONE),
                ]
            }
            fn propagate(&mut self, _: &mut dyn PropagateCtx) {}
        }
        let t =
            c.add_component_oriented(Location::new(0, 200), Direction::East, true, Box::new(Two));
        assert_eq!(c.port_location(t, 1), Location::new(-40, 220)); // was above the origin
        assert_eq!(c.port_location(t, 2), Location::new(-40, 180));
        let r =
            c.add_component_oriented(Location::new(0, 400), Direction::North, true, Box::new(Two));
        assert_eq!(c.port_location(r, 1), Location::new(20, 440));
    }

    #[test]
    fn t_junction_connects() {
        let mut c = Circuit::new();
        c.add_component(
            Location::new(0, 0),
            Direction::East,
            Box::new(Src(Value::TRUE)),
        );
        let inv = c.add_component(Location::new(50, 20), Direction::East, Box::new(Inv));
        c.add_wire(Location::new(0, 0), Location::new(40, 0));
        // input port at (40,20) wired up to (40,0), a mid-point of nothing: endpoint join
        c.add_wire(Location::new(40, 20), Location::new(40, 0));
        c.reset();
        assert_eq!(c.port_value(inv, 0), Value::FALSE);
    }

    #[test]
    fn oscillator_is_detected() {
        // a part that flips its output each time it sees a change, fed back to itself
        struct Flip(bool);
        impl Component for Flip {
            fn name(&self) -> &str {
                "flip"
            }
            fn ports(&self) -> Vec<Port> {
                vec![
                    Port::new(0, 0, PortKind::Output, BitWidth::ONE),
                    Port::new(-10, 0, PortKind::Input, BitWidth::ONE),
                ]
            }
            fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
                self.0 = !self.0;
                ctx.set_port(0, if self.0 { Value::TRUE } else { Value::FALSE }, 1);
            }
        }
        let mut c = Circuit::new();
        c.add_component(Location::new(10, 0), Direction::East, Box::new(Flip(false)));
        c.add_wire(Location::new(10, 0), Location::new(0, 0));
        c.reset();
        assert!(c.is_oscillating());
    }

    #[test]
    fn conflicting_drivers_give_error() {
        let mut c = Circuit::new();
        c.add_component(
            Location::new(0, 0),
            Direction::East,
            Box::new(Src(Value::TRUE)),
        );
        c.add_component(
            Location::new(20, 0),
            Direction::East,
            Box::new(Src(Value::FALSE)),
        );
        c.add_wire(Location::new(0, 0), Location::new(20, 0));
        c.reset();
        assert_eq!(c.value_at(Location::new(0, 0)), Value::ERROR);
    }
}
