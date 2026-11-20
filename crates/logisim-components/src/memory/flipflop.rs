//! D, T, J-K and S-R flip-flops (Java `AbstractFlipFlop` and subclasses).
//!
//! Port order (east-facing, classic layout):
//! `0` Q (0,0) · `1` Q̄ (0,20) · data inputs · clock · clear · preset, where the data inputs are
//! `D` (-40,20) | `T` (-40,20) | `J`,`K` (-40,0),(-40,20) | `S`,`R` (-40,0),(-40,20);
//! the clock is at (-40,0) (one data input) or (-40,10) (two); clear (-10,30); preset (-30,30).
//! Clear (asynchronous reset to 0) has priority over preset.

use logisim_core::{BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{ClockState, Startup, Trigger, FLIPFLOP_DELAY};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlipFlopKind {
    D,
    T,
    Jk,
    Sr,
}

impl FlipFlopKind {
    /// Number of data inputs.
    pub fn inputs(self) -> usize {
        match self {
            FlipFlopKind::D | FlipFlopKind::T => 1,
            FlipFlopKind::Jk | FlipFlopKind::Sr => 2,
        }
    }
    /// D and S-R may be level-triggered (latches); T and J-K are edge-triggered only.
    pub fn allows_level_trigger(self) -> bool {
        matches!(self, FlipFlopKind::D | FlipFlopKind::Sr)
    }
    fn name(self) -> &'static str {
        match self {
            FlipFlopKind::D => "D Flip-Flop",
            FlipFlopKind::T => "T Flip-Flop",
            FlipFlopKind::Jk => "J-K Flip-Flop",
            FlipFlopKind::Sr => "S-R Flip-Flop",
        }
    }

    /// Next state given the data inputs and current state (`Value::UNKNOWN` etc. are ignored
    /// by the caller unless the result is exactly 0 or 1).
    fn compute(self, inputs: &[Value], cur: Value) -> Value {
        match self {
            FlipFlopKind::D => inputs[0],
            FlipFlopKind::T => {
                let cur = if cur == Value::UNKNOWN { Value::FALSE } else { cur };
                if inputs[0] == Value::TRUE {
                    !cur
                } else {
                    cur
                }
            }
            FlipFlopKind::Jk => match (inputs[0], inputs[1]) {
                (Value::FALSE, Value::FALSE) => cur,
                (Value::FALSE, Value::TRUE) => Value::FALSE,
                (Value::TRUE, Value::FALSE) => Value::TRUE,
                (Value::TRUE, Value::TRUE) => !cur,
                _ => Value::UNKNOWN,
            },
            FlipFlopKind::Sr => match (inputs[0], inputs[1]) {
                (Value::FALSE, Value::FALSE) => cur,
                (Value::FALSE, Value::TRUE) => Value::FALSE,
                (Value::TRUE, Value::FALSE) => Value::TRUE,
                (Value::TRUE, Value::TRUE) => Value::ERROR,
                _ => Value::UNKNOWN,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlipFlopOptions {
    pub trigger: Trigger,
    pub startup: Startup,
}

impl Default for FlipFlopOptions {
    fn default() -> Self {
        FlipFlopOptions {
            trigger: Trigger::Rising,
            startup: Startup::Zero,
        }
    }
}

impl FlipFlopOptions {
    pub fn trigger(mut self, t: Trigger) -> Self {
        self.trigger = t;
        self
    }
    pub fn startup(mut self, s: Startup) -> Self {
        self.startup = s;
        self
    }
}

pub struct FlipFlop {
    kind: FlipFlopKind,
    opts: FlipFlopOptions,
    clock: ClockState,
    cur: Value,
}

impl FlipFlop {
    pub fn new(kind: FlipFlopKind) -> Self {
        Self::with_options(kind, FlipFlopOptions::default())
    }
    /// # Panics
    /// If a level trigger is requested for a kind that only supports edges (T, J-K).
    pub fn with_options(kind: FlipFlopKind, opts: FlipFlopOptions) -> Self {
        assert!(
            opts.trigger.is_edge() || kind.allows_level_trigger(),
            "{} only supports edge triggers",
            kind.name()
        );
        let cur = match opts.startup {
            Startup::Zero => Value::FALSE,
            Startup::Unknown => Value::UNKNOWN,
        };
        FlipFlop {
            kind,
            opts,
            clock: ClockState::default(),
            cur,
        }
    }

    pub const Q: usize = 0;
    pub const Q_BAR: usize = 1;
    /// Port index of data input `i` (D/T/J/S = 0, K/R = 1).
    pub const fn data(i: usize) -> usize {
        2 + i
    }
    pub fn clock_port(&self) -> usize {
        2 + self.kind.inputs()
    }
    pub fn clear_port(&self) -> usize {
        3 + self.kind.inputs()
    }
    pub fn preset_port(&self) -> usize {
        4 + self.kind.inputs()
    }
    /// The stored bit.
    pub fn state(&self) -> Value {
        self.cur
    }
}

impl Component for FlipFlop {
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn ports(&self) -> Vec<Port> {
        let one = BitWidth::ONE;
        let n = self.kind.inputs();
        let mut ps = vec![
            Port::new(0, 0, PortKind::Output, one),
            Port::new(0, 20, PortKind::Output, one),
        ];
        if n == 1 {
            ps.push(Port::new(-40, 20, PortKind::Input, one));
            ps.push(Port::new(-40, 0, PortKind::Input, one)); // clock
        } else {
            ps.push(Port::new(-40, 0, PortKind::Input, one));
            ps.push(Port::new(-40, 20, PortKind::Input, one));
            ps.push(Port::new(-40, 10, PortKind::Input, one)); // clock
        }
        ps.push(Port::new(-10, 30, PortKind::Input, one)); // clear
        ps.push(Port::new(-30, 30, PortKind::Input, one)); // preset
        ps
    }

    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let n = self.kind.inputs();
        let triggered = self
            .clock
            .update(ctx.port_value(self.clock_port()), self.opts.trigger);
        if ctx.port_value(self.clear_port()) == Value::TRUE {
            self.cur = Value::FALSE;
        } else if ctx.port_value(self.preset_port()) == Value::TRUE {
            self.cur = Value::TRUE;
        } else if triggered {
            let inputs: Vec<Value> = (0..n).map(|i| ctx.port_value(Self::data(i))).collect();
            let next = self.kind.compute(&inputs, self.cur);
            if next == Value::TRUE || next == Value::FALSE {
                self.cur = next;
            }
        }
        ctx.set_port(Self::Q, self.cur, FLIPFLOP_DELAY);
        ctx.set_port(Self::Q_BAR, !self.cur, FLIPFLOP_DELAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::testutil::Bench;

    const T: Value = Value::TRUE;
    const F: Value = Value::FALSE;

    fn bench(kind: FlipFlopKind, opts: FlipFlopOptions) -> (Bench, FlipFlop) {
        let proto = FlipFlop::with_options(kind, opts.clone());
        let ff = FlipFlop::with_options(kind, opts);
        let mut b = Bench::new(Box::new(ff));
        for i in 0..kind.inputs() {
            b.drive(FlipFlop::data(i), F);
        }
        b.drive(proto.clock_port(), F);
        b.drive(proto.clear_port(), F);
        b.drive(proto.preset_port(), F);
        b.start();
        (b, proto)
    }

    #[test]
    fn d_samples_on_rising_edge_only() {
        let (mut b, p) = bench(FlipFlopKind::D, FlipFlopOptions::default());
        assert_eq!((b.out(0), b.out(1)), (F, T));
        b.set(FlipFlop::data(0), T);
        assert_eq!(b.out(0), F, "data change alone does nothing");
        b.set(p.clock_port(), T);
        assert_eq!((b.out(0), b.out(1)), (T, F));
        b.set(FlipFlop::data(0), F);
        b.set(p.clock_port(), F); // falling edge: no capture
        assert_eq!(b.out(0), T);
        b.pulse(p.clock_port());
        assert_eq!(b.out(0), F);
    }

    #[test]
    fn d_falling_edge_trigger() {
        let (mut b, p) = bench(
            FlipFlopKind::D,
            FlipFlopOptions::default().trigger(Trigger::Falling),
        );
        b.set(FlipFlop::data(0), T);
        b.set(p.clock_port(), T);
        assert_eq!(b.out(0), F);
        b.set(p.clock_port(), F);
        assert_eq!(b.out(0), T);
    }

    #[test]
    fn d_latch_is_transparent_while_enabled() {
        let (mut b, p) = bench(
            FlipFlopKind::D,
            FlipFlopOptions::default().trigger(Trigger::High),
        );
        b.set(p.clock_port(), T);
        b.set(FlipFlop::data(0), T);
        assert_eq!(b.out(0), T);
        b.set(FlipFlop::data(0), F);
        assert_eq!(b.out(0), F);
        b.set(p.clock_port(), F);
        b.set(FlipFlop::data(0), T);
        assert_eq!(b.out(0), F, "holds while disabled");
    }

    #[test]
    fn clear_beats_preset_and_both_are_asynchronous() {
        let (mut b, p) = bench(FlipFlopKind::D, FlipFlopOptions::default());
        b.set(p.preset_port(), T);
        assert_eq!(b.out(0), T);
        b.set(p.preset_port(), F);
        assert_eq!(b.out(0), T, "preset is remembered");
        b.set(p.preset_port(), T);
        b.set(p.clear_port(), T);
        assert_eq!(b.out(0), F, "clear wins");
    }

    #[test]
    fn t_toggles_when_t_high() {
        let (mut b, p) = bench(FlipFlopKind::T, FlipFlopOptions::default());
        b.pulse(p.clock_port());
        assert_eq!(b.out(0), F, "T=0 holds");
        b.set(FlipFlop::data(0), T);
        b.pulse(p.clock_port());
        assert_eq!(b.out(0), T);
        b.pulse(p.clock_port());
        assert_eq!(b.out(0), F);
    }

    #[test]
    fn t_from_unknown_start_treats_state_as_zero() {
        let (mut b, p) = bench(
            FlipFlopKind::T,
            FlipFlopOptions::default().startup(Startup::Unknown),
        );
        assert_eq!(b.out(0), Value::UNKNOWN);
        b.set(FlipFlop::data(0), T);
        b.pulse(p.clock_port());
        assert_eq!(b.out(0), T);
    }

    #[test]
    fn jk_truth_table() {
        // (j, k, state before) -> state after one rising edge
        let cases = [
            (F, F, T, T),
            (F, F, F, F),
            (F, T, T, F),
            (T, F, F, T),
            (T, T, F, T),
            (T, T, T, F),
        ];
        for (j, k, before, after) in cases {
            let (mut b, p) = bench(FlipFlopKind::Jk, FlipFlopOptions::default());
            if before == T {
                b.set(p.preset_port(), T);
                b.set(p.preset_port(), F);
            }
            b.set(FlipFlop::data(0), j);
            b.set(FlipFlop::data(1), k);
            b.pulse(p.clock_port());
            assert_eq!(b.out(0), after, "J={j} K={k} Q={before}");
        }
    }

    #[test]
    fn jk_unknown_input_leaves_state_unchanged() {
        // Java ignores any computed value that is not exactly 0/1
        let (mut b, p) = bench(FlipFlopKind::Jk, FlipFlopOptions::default());
        b.set(FlipFlop::data(0), Value::UNKNOWN);
        b.pulse(p.clock_port());
        assert_eq!(b.out(0), F);
    }

    #[test]
    fn sr_set_reset_and_forbidden_input() {
        let (mut b, p) = bench(FlipFlopKind::Sr, FlipFlopOptions::default());
        b.set(FlipFlop::data(0), T); // S
        b.pulse(p.clock_port());
        assert_eq!(b.out(0), T);
        b.set(FlipFlop::data(0), F);
        b.set(FlipFlop::data(1), T); // R
        b.pulse(p.clock_port());
        assert_eq!(b.out(0), F);
        b.set(FlipFlop::data(0), T); // S=R=1: error, state kept
        b.pulse(p.clock_port());
        assert_eq!(b.out(0), F);
    }

    #[test]
    #[should_panic(expected = "edge triggers")]
    fn jk_rejects_level_trigger() {
        FlipFlop::with_options(
            FlipFlopKind::Jk,
            FlipFlopOptions::default().trigger(Trigger::High),
        );
    }

    #[test]
    fn port_layout() {
        let ps = FlipFlop::new(FlipFlopKind::D).ports();
        assert_eq!(ps.len(), 6);
        assert_eq!((ps[0].offset.x, ps[0].offset.y), (0, 0));
        assert_eq!(FlipFlop::new(FlipFlopKind::Jk).ports().len(), 7);
        assert!(ps.iter().all(|p| p.offset.x % 10 == 0 && p.offset.y % 10 == 0));
    }
}
