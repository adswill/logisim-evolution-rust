//! Shift register (Java `ShiftRegister`).
//!
//! Port order (east-facing, classic layout, translated so the serial output is at (0,0)):
//! `0` serial out · `1` serial in · `2` shift enable · `3` clock · `4` clear · `5` parallel load ·
//! then, when `parallel` is on, for stage `i`: `6+2i` parallel in, `7+2i` parallel out.
//! Without `parallel` only ports 0..=4 exist (5 ports); the load port (5) and stage ports are absent.
//!
//! Stage 0 holds the newest bit; the serial output is the oldest stage (`length-1`).
//! Shift enable is active unless exactly 0. Clear dominates; load dominates shifting.

use logisim_core::{BitWidth, Component, OptionMap, Port, PortKind, PropagateCtx, Value};

use super::register::initial;
use super::{ClockState, Startup, Trigger, SHIFT_DELAY, STARTUPS, TRIGGERS};

pub const OUT: usize = 0;
pub const IN: usize = 1;
pub const SHIFT: usize = 2;
pub const CLOCK: usize = 3;
pub const CLEAR: usize = 4;
pub const LOAD: usize = 5;
/// Port index of the parallel input of stage `i`.
pub const fn parallel_in(i: usize) -> usize {
    6 + 2 * i
}
/// Port index of the parallel output of stage `i`.
pub const fn parallel_out(i: usize) -> usize {
    7 + 2 * i
}

#[derive(Debug, Clone)]
pub struct ShiftRegisterOptions {
    pub width: BitWidth,
    /// Number of stages, 1..=64.
    pub length: usize,
    /// Parallel load/output ports present.
    pub parallel: bool,
    /// Edge trigger only.
    pub trigger: Trigger,
    pub startup: Startup,
}

impl Default for ShiftRegisterOptions {
    fn default() -> Self {
        ShiftRegisterOptions {
            width: BitWidth::ONE,
            length: 8,
            parallel: true,
            trigger: Trigger::Rising,
            startup: Startup::Zero,
        }
    }
}

impl ShiftRegisterOptions {
    pub fn width(mut self, w: u8) -> Self {
        assert!((1..=64).contains(&w), "width must be 1..=64");
        self.width = BitWidth::new(w);
        self
    }
    pub fn length(mut self, n: usize) -> Self {
        assert!((1..=64).contains(&n), "shift register length must be 1..=64");
        self.length = n;
        self
    }
    pub fn parallel(mut self, p: bool) -> Self {
        self.parallel = p;
        self
    }
    /// # Panics
    /// For level triggers, which shift registers do not support.
    pub fn trigger(mut self, t: Trigger) -> Self {
        assert!(t.is_edge(), "shift registers only support edge triggers");
        self.trigger = t;
        self
    }
    pub fn startup(mut self, s: Startup) -> Self {
        self.startup = s;
        self
    }
}

pub struct ShiftRegister {
    opts: ShiftRegisterOptions,
    clock: ClockState,
    /// `stages[0]` is the newest bit, `stages[len-1]` the oldest (serial output).
    stages: Vec<Value>,
}

impl ShiftRegister {
    pub fn new(opts: ShiftRegisterOptions) -> Self {
        let stages = vec![initial(opts.width, opts.startup); opts.length];
        ShiftRegister {
            opts,
            clock: ClockState::default(),
            stages,
        }
    }
    /// Contents of stage `i` (0 = newest).
    pub fn stage(&self, i: usize) -> Value {
        self.stages[i]
    }
}

impl Component for ShiftRegister {
    fn name(&self) -> &str {
        "Shift Register"
    }

    fn options(&self) -> OptionMap {
        let o = &self.opts;
        OptionMap::new()
            .with("width", o.width.get())
            .with("length", o.length)
            .with("parallel", o.parallel)
            .with("trigger", OptionMap::name_of(o.trigger, &TRIGGERS))
            .with("startup", OptionMap::name_of(o.startup, &STARTUPS))
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width", "length", "parallel", "trigger", "startup"])?;
        let mut o = self.opts.clone();
        if let Some(v) = m.int("width", 1, 64)? {
            o.width = BitWidth::new(v as u8);
        }
        if let Some(v) = m.int("length", 1, 64)? {
            o.length = v as usize;
        }
        o.parallel = m.boolean("parallel")?.unwrap_or(o.parallel);
        let trigger = m.choice("trigger", &TRIGGERS)?.unwrap_or(o.trigger);
        if !trigger.is_edge() {
            return Err("shift registers only support rising or falling triggers".into());
        }
        o.trigger = trigger;
        o.startup = m.choice("startup", &STARTUPS)?.unwrap_or(o.startup);
        *self = ShiftRegister::new(o);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let (w, one) = (self.opts.width, BitWidth::ONE);
        let len = self.opts.length as i32;
        let width = if self.opts.parallel { 20 + 10 * len } else { 30 };
        let at = |x: i32, y: i32, k, bits| Port::new(x - width, y, k, bits);
        let mut ps = vec![
            at(width, 0, PortKind::Output, w),
            at(0, 0, PortKind::Input, w),
            at(0, -10, PortKind::Input, one),
            at(0, 10, PortKind::Input, one),
            at(10, 20, PortKind::Input, one),
        ];
        if self.opts.parallel {
            ps.push(at(10, -20, PortKind::Input, one));
            for i in 0..len {
                ps.push(at(20 + 10 * i, -20, PortKind::Input, w));
                ps.push(at(20 + 10 * i, 20, PortKind::Output, w));
            }
        }
        ps
    }

    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let len = self.opts.length;
        let triggered = self.clock.update(ctx.port_value(CLOCK), self.opts.trigger);
        if ctx.port_value(CLEAR) == Value::TRUE {
            self.stages = vec![Value::known(self.opts.width, 0); len];
        } else if triggered {
            if self.opts.parallel && ctx.port_value(LOAD) == Value::TRUE {
                for i in 0..len {
                    self.stages[i] = ctx.port_value(parallel_in(i));
                }
            } else if ctx.port_value(SHIFT) != Value::FALSE {
                self.stages.rotate_right(1);
                self.stages[0] = ctx.port_value(IN);
            }
        }
        ctx.set_port(OUT, self.stages[len - 1], SHIFT_DELAY);
        if self.opts.parallel {
            for i in 0..len {
                ctx.set_port(parallel_out(i), self.stages[i], SHIFT_DELAY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::testutil::Bench;

    const T: Value = Value::TRUE;
    const F: Value = Value::FALSE;

    fn val(w: u8, v: u64) -> Value {
        Value::known(BitWidth::new(w), v)
    }

    fn bench(opts: ShiftRegisterOptions) -> Bench {
        let (w, parallel, len) = (opts.width.get(), opts.parallel, opts.length);
        let mut b = Bench::new(Box::new(ShiftRegister::new(opts)));
        b.drive(IN, val(w, 0));
        b.drive(SHIFT, T);
        b.drive(CLOCK, F);
        b.drive(CLEAR, F);
        if parallel {
            b.drive(LOAD, F);
            for i in 0..len {
                b.drive(parallel_in(i), val(w, 0));
            }
        }
        b.start();
        b
    }

    #[test]
    fn serial_shift_reaches_output_after_length_clocks() {
        let mut b = bench(ShiftRegisterOptions::default().length(3));
        b.set(IN, T);
        b.pulse(CLOCK);
        b.set(IN, F);
        assert_eq!(b.out(OUT), F);
        b.pulse(CLOCK);
        assert_eq!(b.out(OUT), F);
        b.pulse(CLOCK);
        assert_eq!(b.out(OUT), T, "bit emerges after 3 clocks");
        b.pulse(CLOCK);
        assert_eq!(b.out(OUT), F);
    }

    #[test]
    fn parallel_outputs_show_each_stage() {
        let mut b = bench(ShiftRegisterOptions::default().length(3));
        for bit in [T, F, T] {
            b.set(IN, bit);
            b.pulse(CLOCK);
        }
        // newest bit is stage 0
        assert_eq!(b.out(parallel_out(0)), T);
        assert_eq!(b.out(parallel_out(1)), F);
        assert_eq!(b.out(parallel_out(2)), T);
    }

    #[test]
    fn parallel_load_then_shift_out_msb_first() {
        let mut b = bench(ShiftRegisterOptions::default().length(4));
        for (i, v) in [T, T, F, F].into_iter().enumerate() {
            b.set(parallel_in(i), v);
        }
        b.set(LOAD, T);
        b.pulse(CLOCK);
        b.set(LOAD, F);
        let outs: Vec<_> = (0..4).map(|i| b.out(parallel_out(i))).collect();
        assert_eq!(outs, [T, T, F, F], "load is read back unchanged");
        let mut serial = vec![];
        for _ in 0..4 {
            serial.push(b.out(OUT));
            b.pulse(CLOCK);
        }
        assert_eq!(serial, [F, F, T, T], "stage 3 leaves first");
    }

    #[test]
    fn shift_disable_and_clear() {
        let mut b = bench(ShiftRegisterOptions::default().length(2));
        b.set(IN, T);
        b.set(SHIFT, F);
        b.pulse(CLOCK);
        assert_eq!(b.out(parallel_out(0)), F);
        b.set(SHIFT, T);
        b.pulse(CLOCK);
        assert_eq!(b.out(parallel_out(0)), T);
        b.set(CLEAR, T);
        assert_eq!(b.out(parallel_out(0)), F);
    }

    #[test]
    fn multi_bit_and_no_parallel() {
        let mut b = bench(ShiftRegisterOptions::default().width(4).length(2).parallel(false));
        b.set(IN, val(4, 9));
        b.pulse(CLOCK);
        b.set(IN, val(4, 6));
        b.pulse(CLOCK);
        assert_eq!(b.out(OUT), val(4, 9));
        b.pulse(CLOCK);
        assert_eq!(b.out(OUT), val(4, 6));
        assert_eq!(ShiftRegister::new(ShiftRegisterOptions::default().parallel(false)).ports().len(), 5);
    }

    #[test]
    fn port_geometry_puts_serial_out_at_origin() {
        let ps = ShiftRegister::new(ShiftRegisterOptions::default().length(4)).ports();
        assert_eq!(ps.len(), 6 + 2 * 4);
        assert_eq!((ps[OUT].offset.x, ps[OUT].offset.y), (0, 0));
        assert_eq!((ps[IN].offset.x, ps[IN].offset.y), (-60, 0));
        assert!(ps.iter().all(|p| p.offset.x % 10 == 0 && p.offset.y % 10 == 0));
    }
}
