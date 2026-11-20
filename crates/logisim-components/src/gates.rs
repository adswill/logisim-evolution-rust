//! The Gates library: AND, OR, NAND, NOR, XOR, XNOR, NOT, Buffer, controlled buffer/inverter.
//! Port layout matches Logisim: port 0 is the output at (0,0); inputs are 1..=n on the left.

use logisim_core::{BitWidth, Component, Direction, Location, Port, PortKind, PropagateCtx, Value};

/// Gate propagation delay in simulator ticks.
pub const DELAY: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    And,
    Or,
    Nand,
    Nor,
    Xor,
    Xnor,
}

/// How multi-input XOR/XNOR behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XorMode {
    /// True when an odd number of inputs are true (Logisim default).
    #[default]
    Odd,
    /// True when exactly one input is true.
    ExactlyOne,
}

/// What a gate drives for logic 1 / 0 ("open-collector" style outputs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    #[default]
    Normal,
    /// 1 floats (pull-down only).
    ZeroOrFloat,
    /// 0 floats (pull-up only).
    FloatOrOne,
}

/// Whether disconnected / undefined inputs make the output an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UndefinedPolicy {
    /// Unconnected inputs are ignored; floating inputs become errors via the gate math.
    #[default]
    Ignore,
    /// Any unconnected input forces an error output.
    Error,
}

#[derive(Debug, Clone)]
pub struct GateOptions {
    pub inputs: usize,
    pub width: BitWidth,
    /// Bit i set → input i is inverted (bubble on that input).
    pub negated: u64,
    /// Gate body size: 30 (narrow), 50 (medium, default), 70 (wide).
    pub size: i32,
    pub xor_mode: XorMode,
    pub output: OutputMode,
    pub undefined: UndefinedPolicy,
}

impl Default for GateOptions {
    fn default() -> Self {
        GateOptions {
            inputs: 2,
            width: BitWidth::ONE,
            negated: 0,
            size: 50,
            xor_mode: XorMode::Odd,
            output: OutputMode::Normal,
            undefined: UndefinedPolicy::Ignore,
        }
    }
}

impl GateOptions {
    pub fn inputs(mut self, n: usize) -> Self {
        assert!((2..=64).contains(&n), "gates take 2..=64 inputs");
        self.inputs = n;
        self
    }
    pub fn width(mut self, w: u8) -> Self {
        self.width = BitWidth::new(w);
        self
    }
    pub fn negate_input(mut self, i: usize) -> Self {
        self.negated |= 1 << i;
        self
    }
}

fn pull_output(v: Value, mode: OutputMode) -> Value {
    let swap = match mode {
        OutputMode::Normal => return v,
        OutputMode::ZeroOrFloat => Value::TRUE,
        OutputMode::FloatOrOne => Value::FALSE,
    };
    Value::from_bits(
        &v.bits()
            .into_iter()
            .map(|b| if b == swap { Value::UNKNOWN } else { b })
            .collect::<Vec<_>>(),
    )
}

/// Per-bit "exactly one input is true"; any non-0/1 bit makes that output bit an error.
fn exactly_one(inputs: &[Value]) -> Value {
    let width = inputs[0].width().get();
    let bits: Vec<Value> = (0..width)
        .map(|i| {
            let mut count = 0;
            for v in inputs {
                match v.bit(i) {
                    Value::TRUE => count += 1,
                    Value::FALSE => {}
                    _ => return Value::ERROR,
                }
            }
            if count == 1 {
                Value::TRUE
            } else {
                Value::FALSE
            }
        })
        .collect();
    Value::from_bits(&bits)
}

/// Offset of input `index` (0-based) for a gate with `n` inputs, east-facing.
/// Ported from Logisim's `AbstractGate.getInputOffset`.
pub fn input_offset(o: &GateOptions, kind: GateKind, index: usize) -> Location {
    let n = o.inputs as i32;
    let size = o.size;
    let negate_out = matches!(kind, GateKind::Nand | GateKind::Nor | GateKind::Xnor);
    let bonus = if matches!(kind, GateKind::Xor | GateKind::Xnor) {
        10
    } else {
        0
    };
    let axis = size + bonus + if negate_out { 10 } else { 0 };
    let (start, dist, lower_even) = if n <= 3 {
        if size < 40 {
            (-5, 10, 10)
        } else if size < 60 || n <= 2 {
            (-10, 20, 20)
        } else {
            (-15, 30, 30)
        }
    } else {
        (-5, 10, 10)
    };
    let i = index as i32;
    let dy = if n & 1 == 1 {
        start * (n - 1) + dist * i
    } else {
        let mut d = start * n + dist * i;
        if i >= n / 2 {
            d += lower_even;
        }
        if n == 4 && size >= 60 {
            d -= 10;
        }
        d
    };
    let dx = axis + if o.negated >> index & 1 == 1 { 10 } else { 0 };
    Location::new(-dx, dy)
}

pub struct Gate {
    kind: GateKind,
    opts: GateOptions,
}

impl Gate {
    pub fn new(kind: GateKind, opts: GateOptions) -> Self {
        Gate { kind, opts }
    }
    pub fn and(opts: GateOptions) -> Self {
        Gate::new(GateKind::And, opts)
    }
    pub fn or(opts: GateOptions) -> Self {
        Gate::new(GateKind::Or, opts)
    }
    pub fn nand(opts: GateOptions) -> Self {
        Gate::new(GateKind::Nand, opts)
    }
    pub fn nor(opts: GateOptions) -> Self {
        Gate::new(GateKind::Nor, opts)
    }
    pub fn xor(opts: GateOptions) -> Self {
        Gate::new(GateKind::Xor, opts)
    }
    pub fn xnor(opts: GateOptions) -> Self {
        Gate::new(GateKind::Xnor, opts)
    }

    fn compute(&self, inputs: &[Value]) -> Value {
        let fold =
            |f: fn(Value, Value) -> Value| inputs[1..].iter().fold(inputs[0], |a, b| f(a, *b));
        let xor = || match self.opts.xor_mode {
            XorMode::Odd => fold(Value::xor),
            XorMode::ExactlyOne => exactly_one(inputs),
        };
        match self.kind {
            GateKind::And => fold(Value::and),
            GateKind::Nand => !fold(Value::and),
            GateKind::Or => fold(Value::or),
            GateKind::Nor => !fold(Value::or),
            GateKind::Xor => xor(),
            GateKind::Xnor => !xor(),
        }
    }
}

impl Component for Gate {
    fn name(&self) -> &str {
        match self.kind {
            GateKind::And => "AND Gate",
            GateKind::Or => "OR Gate",
            GateKind::Nand => "NAND Gate",
            GateKind::Nor => "NOR Gate",
            GateKind::Xor => "XOR Gate",
            GateKind::Xnor => "XNOR Gate",
        }
    }
    fn ports(&self) -> Vec<Port> {
        let mut p = vec![Port::new(0, 0, PortKind::Output, self.opts.width)];
        for i in 0..self.opts.inputs {
            let o = input_offset(&self.opts, self.kind, i);
            p.push(Port::new(o.x, o.y, PortKind::Input, self.opts.width));
        }
        p
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let mut inputs = Vec::with_capacity(self.opts.inputs);
        let mut error = false;
        for i in 0..self.opts.inputs {
            if ctx.is_port_connected(i + 1) {
                let v = ctx.port_value(i + 1);
                inputs.push(if self.opts.negated >> i & 1 == 1 {
                    !v
                } else {
                    v
                });
            } else if self.opts.undefined == UndefinedPolicy::Error {
                error = true;
            }
        }
        let out = if inputs.is_empty() || error {
            Value::error(self.opts.width)
        } else {
            pull_output(self.compute(&inputs), self.opts.output)
        };
        ctx.set_port(0, out, DELAY);
    }
}

/// Buffer and NOT gate: one input at (-20,0)/(−size), output at (0,0).
pub struct Buffer {
    pub inverting: bool,
    pub width: BitWidth,
    pub undefined: UndefinedPolicy,
}

impl Buffer {
    pub fn new(width: u8) -> Self {
        Buffer {
            inverting: false,
            width: BitWidth::new(width),
            undefined: UndefinedPolicy::Ignore,
        }
    }
    pub fn not(width: u8) -> Self {
        Buffer {
            inverting: true,
            ..Buffer::new(width)
        }
    }
}

impl Component for Buffer {
    fn name(&self) -> &str {
        if self.inverting {
            "NOT Gate"
        } else {
            "Buffer"
        }
    }
    fn ports(&self) -> Vec<Port> {
        // NOT has a bubble, so its input sits 10 further out (30 vs 20 at medium size)
        let x = if self.inverting { -30 } else { -20 };
        vec![
            Port::new(0, 0, PortKind::Output, self.width),
            Port::new(x, 0, PortKind::Input, self.width),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let v = ctx.port_value(1);
        let out = if self.inverting { !v } else { v };
        // repair: wrong width or undefined bits become errors when the policy says so
        let out = if self.undefined == UndefinedPolicy::Error {
            if out.width() == self.width && out.is_fully_defined() {
                out
            } else {
                let bits: Vec<Value> = (0..self.width.get())
                    .map(|i| {
                        let b = if i < out.width().get() {
                            out.bit(i)
                        } else {
                            Value::ERROR
                        };
                        if b.is_fully_defined() {
                            b
                        } else {
                            Value::ERROR
                        }
                    })
                    .collect();
                Value::from_bits(&bits)
            }
        } else {
            out
        };
        ctx.set_port(0, out, DELAY);
    }
}

/// Controlled buffer / inverter (tri-state). Ports: 0 out, 1 data in, 2 enable (1 bit).
/// Enable true passes data; false floats; error/unknown handled as in Logisim.
pub struct ControlledBuffer {
    pub inverting: bool,
    pub width: BitWidth,
    pub undefined: UndefinedPolicy,
    pub facing: Direction,
}

impl ControlledBuffer {
    pub fn buffer(width: u8) -> Self {
        ControlledBuffer {
            inverting: false,
            width: BitWidth::new(width),
            undefined: UndefinedPolicy::Ignore,
            facing: Direction::East,
        }
    }
    pub fn inverter(width: u8) -> Self {
        ControlledBuffer {
            inverting: true,
            ..ControlledBuffer::buffer(width)
        }
    }
}

impl Component for ControlledBuffer {
    fn name(&self) -> &str {
        if self.inverting {
            "Controlled Inverter"
        } else {
            "Controlled Buffer"
        }
    }
    fn ports(&self) -> Vec<Port> {
        let x = if self.inverting { -30 } else { -20 };
        vec![
            Port::new(0, 0, PortKind::Output, self.width),
            Port::new(x, 0, PortKind::Input, self.width),
            Port::new(-10, -10, PortKind::Input, BitWidth::ONE),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let control = ctx.port_value(2);
        let out = if control == Value::TRUE {
            let d = ctx.port_value(1);
            if self.inverting {
                !d
            } else {
                d
            }
        } else if control == Value::ERROR
            || ((control == Value::UNKNOWN || control == Value::NIL)
                && self.undefined == UndefinedPolicy::Error)
        {
            Value::error(self.width)
        } else {
            Value::unknown(self.width)
        };
        ctx.set_port(0, out, DELAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiring::Pin;
    use logisim_core::{Circuit, Direction};

    /// Wire pins to a 2-input gate, return the output for the given inputs.
    fn eval2(make: fn(GateOptions) -> Gate, a: Value, b: Value) -> Value {
        let mut c = Circuit::new();
        let g = c.add_component(
            Location::new(100, 0),
            Direction::East,
            Box::new(make(GateOptions::default())),
        );
        for (i, v) in [a, b].into_iter().enumerate() {
            let pin = c.add_component(
                Location::new(0, 40 * i as i32),
                Direction::East,
                Box::new(Pin::new(v)),
            );
            let to = c.port_location(g, i + 1);
            let from = c.port_location(pin, 0);
            // L-shaped route: horizontal then vertical
            c.add_wire(from, Location::new(to.x - 5, from.y));
            c.add_wire(
                Location::new(to.x - 5, from.y),
                Location::new(to.x - 5, to.y),
            );
            c.add_wire(Location::new(to.x - 5, to.y), to);
        }
        c.reset();
        c.port_value(g, 0)
    }

    fn table(make: fn(GateOptions) -> Gate, expect: [bool; 4]) {
        for (i, want) in expect.into_iter().enumerate() {
            let (a, b) = (i & 2 != 0, i & 1 != 0);
            let v = |x| if x { Value::TRUE } else { Value::FALSE };
            let want = v(want);
            assert_eq!(eval2(make, v(a), v(b)), want, "inputs {a} {b}");
        }
    }

    #[test]
    fn truth_tables() {
        table(Gate::and, [false, false, false, true]);
        table(Gate::or, [false, true, true, true]);
        table(Gate::nand, [true, true, true, false]);
        table(Gate::nor, [true, false, false, false]);
        table(Gate::xor, [false, true, true, false]);
        table(Gate::xnor, [true, false, false, true]);
    }

    #[test]
    fn unknown_input_semantics() {
        assert_eq!(eval2(Gate::and, Value::FALSE, Value::UNKNOWN), Value::FALSE);
        assert_eq!(eval2(Gate::and, Value::TRUE, Value::UNKNOWN), Value::ERROR);
        assert_eq!(eval2(Gate::or, Value::TRUE, Value::UNKNOWN), Value::TRUE);
        assert_eq!(eval2(Gate::nand, Value::FALSE, Value::UNKNOWN), Value::TRUE);
    }

    #[test]
    fn three_input_xor_modes() {
        let t = Value::TRUE;
        let mk = |mode| {
            Gate::xor(GateOptions {
                xor_mode: mode,
                ..GateOptions::default().inputs(3)
            })
        };
        let run = |g: Gate| {
            let mut c = Circuit::new();
            let id = c.add_component(Location::new(100, 0), Direction::East, Box::new(g));
            for i in 0..3 {
                let pin = c.add_component(
                    Location::new(0, 40 * i),
                    Direction::East,
                    Box::new(Pin::new(t)),
                );
                let to = c.port_location(id, i as usize + 1);
                let from = c.port_location(pin, 0);
                c.add_wire(from, Location::new(to.x - 5, from.y));
                c.add_wire(
                    Location::new(to.x - 5, from.y),
                    Location::new(to.x - 5, to.y),
                );
                c.add_wire(Location::new(to.x - 5, to.y), to);
            }
            c.reset();
            c.port_value(id, 0)
        };
        assert_eq!(run(mk(XorMode::Odd)), Value::TRUE); // three trues → odd
        assert_eq!(run(mk(XorMode::ExactlyOne)), Value::FALSE);
    }

    #[test]
    fn bus_gate_and_negated_input() {
        let w = BitWidth::new(4);
        let mut c = Circuit::new();
        let opts = GateOptions::default().width(4).negate_input(1);
        let g = c.add_component(
            Location::new(100, 0),
            Direction::East,
            Box::new(Gate::and(opts)),
        );
        for (i, v) in [0b1100u64, 0b1010].into_iter().enumerate() {
            let pin = c.add_component(
                Location::new(0, 40 * i as i32),
                Direction::East,
                Box::new(Pin::new(Value::known(w, v))),
            );
            let to = c.port_location(g, i + 1);
            let from = c.port_location(pin, 0);
            c.add_wire(from, Location::new(to.x - 5, from.y));
            c.add_wire(
                Location::new(to.x - 5, from.y),
                Location::new(to.x - 5, to.y),
            );
            c.add_wire(Location::new(to.x - 5, to.y), to);
        }
        c.reset();
        // 1100 & !1010 = 0100
        assert_eq!(c.port_value(g, 0).to_u64(), Some(0b0100));
    }

    #[test]
    fn not_and_buffer_and_pull_output() {
        assert_eq!(
            pull_output(Value::TRUE, OutputMode::ZeroOrFloat),
            Value::UNKNOWN
        );
        assert_eq!(
            pull_output(Value::FALSE, OutputMode::ZeroOrFloat),
            Value::FALSE
        );
        assert_eq!(
            pull_output(Value::FALSE, OutputMode::FloatOrOne),
            Value::UNKNOWN
        );
        let b = Buffer::not(1);
        assert_eq!(b.ports()[1].offset, Location::new(-30, 0));
        assert_eq!(Buffer::new(8).ports()[0].width.get(), 8);
    }

    #[test]
    fn controlled_buffer_tristate() {
        let run = |inverting: bool, en: Value, data: Value| {
            let mut c = Circuit::new();
            let cb = if inverting {
                ControlledBuffer::inverter(1)
            } else {
                ControlledBuffer::buffer(1)
            };
            let id = c.add_component(Location::new(100, 0), Direction::East, Box::new(cb));
            for (port, v, y) in [(1usize, data, 0), (2, en, -40)] {
                let pin =
                    c.add_component(Location::new(0, y), Direction::East, Box::new(Pin::new(v)));
                let to = c.port_location(id, port);
                let from = c.port_location(pin, 0);
                c.add_wire(from, Location::new(to.x - 5, from.y));
                c.add_wire(
                    Location::new(to.x - 5, from.y),
                    Location::new(to.x - 5, to.y),
                );
                c.add_wire(Location::new(to.x - 5, to.y), to);
            }
            c.reset();
            c.port_value(id, 0)
        };
        assert_eq!(run(false, Value::TRUE, Value::TRUE), Value::TRUE);
        assert_eq!(run(true, Value::TRUE, Value::TRUE), Value::FALSE);
        assert_eq!(run(false, Value::FALSE, Value::TRUE), Value::UNKNOWN);
        assert_eq!(run(false, Value::ERROR, Value::TRUE), Value::ERROR);
    }

    /// SR latch from two cross-coupled NOR gates: set, hold, reset, hold.
    #[test]
    fn sr_latch_holds_state() {
        let mut c = Circuit::new();
        let nor = || Box::new(Gate::nor(GateOptions::default()));
        let g1 = c.add_component(Location::new(100, 0), Direction::East, nor()); // Q
        let g2 = c.add_component(Location::new(100, 100), Direction::East, nor()); // Q'
        let (rp, sp) = (Pin::bit(false), Pin::bit(false));
        let (rh, sh) = (rp.handle(), sp.handle());
        let r = c.add_component(Location::new(0, 0), Direction::East, Box::new(rp));
        let s = c.add_component(Location::new(0, 100), Direction::East, Box::new(sp));
        let route = |c: &mut Circuit, from: Location, to: Location, jog: i32| {
            c.add_wire(from, Location::new(jog, from.y));
            c.add_wire(Location::new(jog, from.y), Location::new(jog, to.y));
            c.add_wire(Location::new(jog, to.y), to);
        };
        let p = |c: &Circuit, id, i| c.port_location(id, i);
        let (r_o, s_o) = (p(&c, r, 0), p(&c, s, 0));
        let (g1_o, g1_a, g1_b) = (p(&c, g1, 0), p(&c, g1, 1), p(&c, g1, 2));
        let (g2_o, g2_a, g2_b) = (p(&c, g2, 0), p(&c, g2, 1), p(&c, g2, 2));
        route(&mut c, r_o, g1_a, 20);
        route(&mut c, s_o, g2_b, 30);
        route(&mut c, g1_o, g2_a, 150);
        route(&mut c, g2_o, g1_b, 160);

        let q = |c: &mut Circuit| (c.port_value(g1, 0), c.port_value(g2, 0));
        // set
        sh.set(Value::TRUE);
        c.reset();
        assert_eq!(q(&mut c), (Value::TRUE, Value::FALSE));
        // hold after S drops
        sh.set(Value::FALSE);
        c.refresh(s);
        assert_eq!(q(&mut c), (Value::TRUE, Value::FALSE));
        assert!(!c.is_oscillating());
        // reset
        rh.set(Value::TRUE);
        c.refresh(r);
        assert_eq!(q(&mut c), (Value::FALSE, Value::TRUE));
        // hold
        rh.set(Value::FALSE);
        c.refresh(r);
        assert_eq!(q(&mut c), (Value::FALSE, Value::TRUE));
    }
}
