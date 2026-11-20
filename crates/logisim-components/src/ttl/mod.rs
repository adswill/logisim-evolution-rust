//! TTL 74xx chip library (port of `com.cburch.logisim.std.ttl`).
//!
//! Every chip is a DIP package. Logic is written against **datasheet pin numbers** (1-based) via
//! [`Pins`], so it can be checked against a datasheet directly.
//!
//! Port layout (chip facing east, `n` = pin count, body height 60): pin `p` (1-based, `i = p - 1`)
//! sits at `(i*20+10, height-30)` for `i < n/2` (bottom row, left to right) and at
//! `(n*10 - (i-n/2)*20 - 10, -30)` on the top row (right to left), as in Logisim's
//! `AbstractTtlGate`. Port order = pin order, skipping unused and power pins; with
//! [`TtlOptions::vcc_gnd_ports`] on (option `vcc_gnd_ports`), two ports (GND, then VCC) are
//! appended and the outputs go to X unless GND = 0 and VCC = 1.
//!
//! Chips with open-collector outputs drive `0` or X (floating), never `1`.

use crate::registry::{Library, PartDesc};
use std::cell::Cell;
use std::rc::Rc;
use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

pub mod buffers;
pub mod counters;
pub mod decoders;
pub mod flipflops;
pub mod gates;
pub mod misc;
pub mod registers;

/// Default chip height (Logisim `DEFAULT_HEIGHT`).
pub const DEFAULT_HEIGHT: i32 = 60;

/// Static description of a package.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub name: &'static str,
    pub pins: u8,
    pub outputs: &'static [u8],
    pub inouts: &'static [u8],
    pub unused: &'static [u8],
    pub vcc: u8,
    pub gnd: u8,
    pub height: i32,
}

impl Spec {
    /// Standard package: VCC on the last pin, GND on the middle pin.
    pub const fn new(name: &'static str, pins: u8, outputs: &'static [u8]) -> Spec {
        Spec {
            name,
            pins,
            outputs,
            inouts: &[],
            unused: &[],
            vcc: pins,
            gnd: pins / 2,
            height: DEFAULT_HEIGHT,
        }
    }
    pub const fn unused(mut self, u: &'static [u8]) -> Spec {
        self.unused = u;
        self
    }
    pub const fn inouts(mut self, u: &'static [u8]) -> Spec {
        self.inouts = u;
        self
    }
    pub const fn power(mut self, vcc: u8, gnd: u8) -> Spec {
        self.vcc = vcc;
        self.gnd = gnd;
        self
    }
    pub const fn height(mut self, h: i32) -> Spec {
        self.height = h;
        self
    }
    fn is_signal(&self, p: u8) -> bool {
        p != self.vcc && p != self.gnd && !self.unused.contains(&p)
    }
}

/// Options shared by all TTL chips.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TtlOptions {
    /// Show explicit VCC/GND ports and require them to be connected (default off).
    pub vcc_gnd_ports: bool,
}

impl TtlOptions {
    pub fn vcc_gnd_ports(mut self, on: bool) -> Self {
        self.vcc_gnd_ports = on;
        self
    }
}

/// Datasheet-pin view of the chip handed to the logic.
pub struct Pins<'a> {
    ctx: &'a mut dyn PropagateCtx,
    map: &'a [Option<usize>],
}

impl Pins<'_> {
    /// Value on datasheet pin `p` (X if the pin has no port).
    pub fn get(&self, p: u8) -> Value {
        match self.map[p as usize] {
            Some(i) => self.ctx.port_value(i),
            None => Value::UNKNOWN,
        }
    }
    /// True only for a clean logic 1.
    pub fn hi(&self, p: u8) -> bool {
        self.get(p) == Value::TRUE
    }
    /// True only for a clean logic 0.
    pub fn lo(&self, p: u8) -> bool {
        self.get(p) == Value::FALSE
    }
    pub fn set(&mut self, p: u8, v: Value, delay: u32) {
        if let Some(i) = self.map[p as usize] {
            self.ctx.set_port(i, v, delay.max(1));
        }
    }
    pub fn set_bool(&mut self, p: u8, b: bool, delay: u32) {
        self.set(p, bit(b), delay);
    }
    /// Drive a pin as an open-collector output: low drives 0, high floats (X).
    pub fn set_open(&mut self, p: u8, high: bool, delay: u32) {
        self.set(p, if high { Value::UNKNOWN } else { Value::FALSE }, delay);
    }
    /// Read several pins as a number, first pin = least significant bit; None if any is not 0/1.
    pub fn read(&self, pins: &[u8]) -> Option<u64> {
        let mut v = 0u64;
        for (i, &p) in pins.iter().enumerate() {
            match self.get(p) {
                Value::TRUE => v |= 1 << i,
                Value::FALSE => {}
                _ => return None,
            }
        }
        Some(v)
    }
    /// Drive several pins from a number (first pin = LSB).
    pub fn write(&mut self, pins: &[u8], v: u64, delay: u32) {
        for (i, &p) in pins.iter().enumerate() {
            self.set_bool(p, (v >> i) & 1 == 1, delay);
        }
    }
    pub fn write_unknown(&mut self, pins: &[u8], delay: u32) {
        for &p in pins {
            self.set(p, Value::UNKNOWN, delay);
        }
    }
    pub fn write_error(&mut self, pins: &[u8], delay: u32) {
        for &p in pins {
            self.set(p, Value::ERROR, delay);
        }
    }
}

/// `true` → 1, `false` → 0.
pub fn bit(b: bool) -> Value {
    if b {
        Value::TRUE
    } else {
        Value::FALSE
    }
}

/// Edge/level clock detection helper (Logisim `ClockState`).
#[derive(Debug, Clone, Copy)]
pub struct Edge {
    last: Value,
}

/// Which clock transition fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Rising,
    Falling,
    High,
    Low,
}

impl Default for Edge {
    fn default() -> Self {
        Edge { last: Value::FALSE }
    }
}

impl Edge {
    pub fn new() -> Self {
        Self::default()
    }
    /// Feed the current clock; true if it triggered.
    pub fn update(&mut self, now: Value, trig: Trigger) -> bool {
        let old = self.last;
        if now == Value::NIL {
            return false;
        }
        self.last = now;
        match trig {
            Trigger::Rising => old == Value::FALSE && now == Value::TRUE,
            Trigger::Falling => old == Value::TRUE && now == Value::FALSE,
            Trigger::High => now == Value::TRUE,
            Trigger::Low => now == Value::FALSE,
        }
    }
    pub fn rising(&mut self, now: Value) -> bool {
        self.update(now, Trigger::Rising)
    }
    pub fn falling(&mut self, now: Value) -> bool {
        self.update(now, Trigger::Falling)
    }
}

type Logic = Box<dyn FnMut(&mut Pins)>;

/// An extra integer option (e.g. a monostable's pulse length) shared with the chip's logic.
struct IntOption {
    key: &'static str,
    cell: Rc<Cell<i64>>,
    lo: i64,
    hi: i64,
}

/// A TTL chip: package description + logic closure (which owns any state).
pub struct Ttl {
    spec: Spec,
    opts: TtlOptions,
    /// datasheet pin -> port index
    map: Vec<Option<usize>>,
    ports: Vec<Port>,
    logic: Logic,
    tick: Option<Logic>,
    int_option: Option<IntOption>,
}

impl Ttl {
    pub fn new(spec: Spec, logic: impl FnMut(&mut Pins) + 'static) -> Ttl {
        Self::with_options(spec, TtlOptions::default(), logic)
    }

    pub fn with_options(
        spec: Spec,
        opts: TtlOptions,
        logic: impl FnMut(&mut Pins) + 'static,
    ) -> Ttl {
        let mut t = Ttl {
            spec,
            opts,
            map: vec![],
            ports: vec![],
            logic: Box::new(logic),
            tick: None,
            int_option: None,
        };
        t.layout();
        t
    }

    /// (Re)build the pin -> port map and port list from the spec and options.
    fn layout(&mut self) {
        let spec = self.spec;
        let n = spec.pins as i32;
        let mut map = vec![None; spec.pins as usize + 1];
        let mut ports = vec![];
        let pos = |i: i32| -> (i32, i32) {
            if i < n / 2 {
                (i * 20 + 10, spec.height - 30)
            } else {
                (n * 10 - (i - n / 2) * 20 - 10, -30)
            }
        };
        let mut power = vec![];
        for p in 1..=spec.pins {
            let (x, y) = pos(p as i32 - 1);
            if spec.unused.contains(&p) {
                continue;
            }
            if p == spec.gnd || p == spec.vcc {
                if self.opts.vcc_gnd_ports {
                    power.push((p, x, y));
                }
                continue;
            }
            let kind = if spec.outputs.contains(&p) {
                PortKind::Output
            } else if spec.inouts.contains(&p) {
                PortKind::InOut
            } else {
                PortKind::Input
            };
            map[p as usize] = Some(ports.len());
            ports.push(Port::new(x, y, kind, BitWidth::ONE));
        }
        // power ports: GND first, then VCC
        power.sort_by_key(|&(p, ..)| p != spec.gnd);
        for (p, x, y) in power {
            map[p as usize] = Some(ports.len());
            ports.push(Port::new(x, y, PortKind::Input, BitWidth::ONE));
        }
        self.map = map;
        self.ports = ports;
    }

    /// Run `f` on every simulation clock tick (for time-based parts such as monostables).
    pub fn on_tick(mut self, f: impl FnMut(&mut Pins) + 'static) -> Ttl {
        self.tick = Some(Box::new(f));
        self
    }

    /// Expose one integer option `key` (range `lo..=hi`) backed by `cell`.
    pub fn with_int_option(mut self, key: &'static str, cell: Rc<Cell<i64>>, lo: i64, hi: i64) -> Ttl {
        self.int_option = Some(IntOption { key, cell, lo, hi });
        self
    }

    pub fn spec(&self) -> &Spec {
        &self.spec
    }
    pub fn options(&self) -> TtlOptions {
        self.opts
    }
    /// Port index of a datasheet pin, if it has one.
    pub fn port_of_pin(&self, pin: u8) -> Option<usize> {
        self.map.get(pin as usize).copied().flatten()
    }
}

impl Component for Ttl {
    fn name(&self) -> &str {
        self.spec.name
    }
    fn ports(&self) -> Vec<Port> {
        self.ports.clone()
    }
    fn options(&self) -> OptionMap {
        let mut m = OptionMap::new().with("vcc_gnd_ports", self.opts.vcc_gnd_ports);
        if let Some(o) = &self.int_option {
            m.set(o.key, o.cell.get());
        }
        m
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        let mut v = vec![OptionSpec::boolean("vcc_gnd_ports", "Show Vcc and GND ports", false)];
        if let Some(o) = &self.int_option {
            let default = o.cell.get();
            v.push(OptionSpec::int(o.key, "Pulse length (ticks)", o.lo, o.hi, default));
        }
        v
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        let mut known = vec!["vcc_gnd_ports"];
        known.extend(self.int_option.as_ref().map(|o| o.key));
        m.reject_unknown(&known)?;
        if let Some(b) = m.boolean("vcc_gnd_ports")? {
            self.opts.vcc_gnd_ports = b;
        }
        if let Some(o) = &self.int_option {
            if let Some(v) = m.int(o.key, o.lo, o.hi)? {
                o.cell.set(v);
            }
        }
        self.layout();
        Ok(())
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let mut pins = Pins {
            ctx,
            map: &self.map,
        };
        if self.opts.vcc_gnd_ports
            && (pins.get(self.spec.gnd) != Value::FALSE || pins.get(self.spec.vcc) != Value::TRUE)
        {
            for p in 1..=self.spec.pins {
                if self.spec.is_signal(p) && self.spec.outputs.contains(&p) {
                    pins.set(p, Value::UNKNOWN, 1);
                }
            }
            return;
        }
        (self.logic)(&mut pins);
    }
    fn tick(&mut self, ctx: &mut dyn PropagateCtx) {
        if let Some(f) = self.tick.as_mut() {
            let mut pins = Pins {
                ctx,
                map: &self.map,
            };
            f(&mut pins);
        }
    }
}

macro_rules! chips {
    ($($name:literal => $f:path),* $(,)?) => {
        vec![$(PartDesc { name: $name, make: || Box::new($f()) }),*]
    };
}
pub(crate) use chips;

/// The TTL library.
pub fn library() -> Library {
    let mut parts = vec![];
    parts.extend(gates::parts());
    parts.extend(decoders::parts());
    parts.extend(flipflops::parts());
    parts.extend(counters::parts());
    parts.extend(registers::parts());
    parts.extend(buffers::parts());
    parts.extend(misc::parts());
    parts.sort_by_key(|p| p.name.parse::<u32>().unwrap_or(u32::MAX));
    Library { name: "TTL", parts }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::wiring::{Pin, PinHandle};
    use logisim_core::{Circuit, CompId, Direction, Location};
    use std::collections::HashMap;

    /// Test rig: a chip with a `Pin` on every input/inout, readable on every pin.
    pub struct Rig {
        pub c: Circuit,
        chip: CompId,
        name: &'static str,
        pins: HashMap<u8, (CompId, PinHandle)>,
        map: Vec<Option<usize>>,
    }

    impl Rig {
        pub fn new(chip: Ttl) -> Rig {
            let spec = *chip.spec();
            let map = chip.map.clone();
            let name = spec.name;
            let ports = chip.ports();
            let mut c = Circuit::new();
            let chip_id = c.add_component(Location::new(200, 200), Direction::East, Box::new(chip));
            let mut pins = HashMap::new();
            for p in 1..=spec.pins {
                let Some(pi) = map[p as usize] else { continue };
                if spec.outputs.contains(&p) {
                    continue;
                }
                let loc = c.port_location(chip_id, pi);
                let up = ports[pi].offset.y < 0;
                let far = Location::new(loc.x, loc.y + if up { -30 } else { 30 });
                let pin = Pin::new(Value::FALSE);
                let h = pin.handle();
                let id = c.add_component(far, Direction::East, Box::new(pin));
                c.add_wire(far, loc);
                pins.insert(p, (id, h));
            }
            c.reset();
            Rig {
                c,
                chip: chip_id,
                name,
                pins,
                map,
            }
        }
        pub fn name(&self) -> &'static str {
            self.name
        }
        pub fn set(&mut self, p: u8, v: Value) {
            let (id, h) = &self.pins[&p];
            h.set(v);
            let id = *id;
            self.c.refresh(id);
        }
        pub fn put(&mut self, p: u8, b: bool) {
            self.set(p, bit(b));
        }
        /// Set several pins (LSB first) from a number.
        pub fn put_n(&mut self, ps: &[u8], v: u64) {
            for (i, &p) in ps.iter().enumerate() {
                let (id, h) = &self.pins[&p];
                h.set(bit((v >> i) & 1 == 1));
                let id = *id;
                self.c.refresh(id);
            }
        }
        pub fn get(&mut self, p: u8) -> Value {
            self.c.port_value(self.chip, self.map[p as usize].unwrap())
        }
        pub fn hi(&mut self, p: u8) -> bool {
            self.get(p) == Value::TRUE
        }
        pub fn read_n(&mut self, ps: &[u8]) -> Option<u64> {
            let mut v = 0;
            for (i, &p) in ps.iter().enumerate() {
                match self.get(p) {
                    Value::TRUE => v |= 1 << i,
                    Value::FALSE => {}
                    _ => return None,
                }
            }
            Some(v)
        }
        /// Clock pulse: 0 -> 1 -> 0 on `p`.
        pub fn pulse(&mut self, p: u8) {
            self.put(p, false);
            self.put(p, true);
            self.put(p, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_round_trips_its_options() {
        let lib = library();
        assert_eq!(lib.parts.len(), 85);
        for p in &lib.parts {
            let part = (p.make)();
            assert_eq!(part.name(), p.name);
            let saved = part.options();
            let mut fresh = (p.make)();
            fresh.set_options(&saved).unwrap_or_else(|e| panic!("{}: {e}", p.name));
            assert_eq!(fresh.options(), saved, "{}", p.name);
            // with power ports on, ports gain GND and VCC
            let mut powered = (p.make)();
            powered
                .set_options(&OptionMap::new().with("vcc_gnd_ports", true))
                .unwrap();
            assert_eq!(powered.ports().len(), part.ports().len() + 2, "{}", p.name);
            assert!(fresh.set_options(&OptionMap::new().with("bogus", true)).is_err());
        }
    }

    #[test]
    fn option_specs_match_options() {
        for p in &library().parts {
            logisim_core::validate_specs(&mut *(p.make)()).unwrap_or_else(|e| panic!("{e}"));
        }
    }
}
