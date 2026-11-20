//! Register (Java `Register`).
//!
//! Port order (east-facing, classic layout): `0` Q (0,0) · `1` D (-30,0) · `2` clock (-20,20) ·
//! `3` clear (-10,20) · `4` enable (-30,10). Q and D are `width` bits wide, the rest 1 bit.
//! Enable is active unless it is exactly 0 (so an unconnected enable still loads).

use logisim_core::{BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{ClockState, Startup, Trigger, REGISTER_DELAY};

pub const Q: usize = 0;
pub const D: usize = 1;
pub const CLOCK: usize = 2;
pub const CLEAR: usize = 3;
pub const ENABLE: usize = 4;

#[derive(Debug, Clone)]
pub struct RegisterOptions {
    pub width: BitWidth,
    pub trigger: Trigger,
    pub startup: Startup,
}

impl Default for RegisterOptions {
    fn default() -> Self {
        RegisterOptions {
            width: BitWidth::new(8),
            trigger: Trigger::Rising,
            startup: Startup::Zero,
        }
    }
}

impl RegisterOptions {
    pub fn width(mut self, w: u8) -> Self {
        assert!((1..=64).contains(&w), "register width must be 1..=64");
        self.width = BitWidth::new(w);
        self
    }
    pub fn trigger(mut self, t: Trigger) -> Self {
        self.trigger = t;
        self
    }
    pub fn startup(mut self, s: Startup) -> Self {
        self.startup = s;
        self
    }
}

pub struct Register {
    opts: RegisterOptions,
    clock: ClockState,
    value: Value,
}

pub(crate) fn initial(width: BitWidth, startup: Startup) -> Value {
    match startup {
        Startup::Zero => Value::known(width, 0),
        Startup::Unknown => Value::unknown(width),
    }
}

impl Register {
    pub fn new(opts: RegisterOptions) -> Self {
        let value = initial(opts.width, opts.startup);
        Register {
            opts,
            clock: ClockState::default(),
            value,
        }
    }
    /// The stored value.
    pub fn value(&self) -> Value {
        self.value
    }
}

impl Component for Register {
    fn name(&self) -> &str {
        "Register"
    }
    fn ports(&self) -> Vec<Port> {
        let (w, one) = (self.opts.width, BitWidth::ONE);
        vec![
            Port::new(0, 0, PortKind::Output, w),
            Port::new(-30, 0, PortKind::Input, w),
            Port::new(-20, 20, PortKind::Input, one),
            Port::new(-10, 20, PortKind::Input, one),
            Port::new(-30, 10, PortKind::Input, one),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let triggered = self.clock.update(ctx.port_value(CLOCK), self.opts.trigger);
        if ctx.port_value(CLEAR) == Value::TRUE {
            self.value = Value::known(self.opts.width, 0);
        } else if triggered && ctx.port_value(ENABLE) != Value::FALSE {
            self.value = ctx.port_value(D);
        }
        ctx.set_port(Q, self.value, REGISTER_DELAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::testutil::Bench;

    fn val(w: u8, v: u64) -> Value {
        Value::known(BitWidth::new(w), v)
    }

    fn bench(opts: RegisterOptions) -> Bench {
        let w = opts.width.get();
        let mut b = Bench::new(Box::new(Register::new(opts)));
        b.drive(D, val(w, 0));
        b.drive(CLOCK, Value::FALSE);
        b.drive(CLEAR, Value::FALSE);
        b.drive(ENABLE, Value::TRUE);
        b.start();
        b
    }

    #[test]
    fn loads_on_rising_edge() {
        let mut b = bench(RegisterOptions::default());
        assert_eq!(b.out(Q), val(8, 0));
        b.set(D, val(8, 0xA5));
        assert_eq!(b.out(Q), val(8, 0));
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(8, 0xA5));
        b.set(D, val(8, 0x11));
        assert_eq!(b.out(Q), val(8, 0xA5), "holds between edges");
    }

    #[test]
    fn enable_low_blocks_load() {
        let mut b = bench(RegisterOptions::default().width(4));
        b.set(D, val(4, 9));
        b.set(ENABLE, Value::FALSE);
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(4, 0));
        b.set(ENABLE, Value::TRUE);
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(4, 9));
    }

    #[test]
    fn clear_is_asynchronous_and_dominant() {
        let mut b = bench(RegisterOptions::default().width(4));
        b.set(D, val(4, 7));
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(4, 7));
        b.set(CLEAR, Value::TRUE);
        assert_eq!(b.out(Q), val(4, 0));
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(4, 0), "clock ignored while clearing");
    }

    #[test]
    fn falling_edge_and_latch_modes() {
        let mut b = bench(RegisterOptions::default().width(4).trigger(Trigger::Falling));
        b.set(D, val(4, 3));
        b.set(CLOCK, Value::TRUE);
        assert_eq!(b.out(Q), val(4, 0));
        b.set(CLOCK, Value::FALSE);
        assert_eq!(b.out(Q), val(4, 3));

        let mut l = bench(RegisterOptions::default().width(4).trigger(Trigger::High));
        l.set(CLOCK, Value::TRUE);
        l.set(D, val(4, 5));
        assert_eq!(l.out(Q), val(4, 5), "transparent while high");
        l.set(CLOCK, Value::FALSE);
        l.set(D, val(4, 6));
        assert_eq!(l.out(Q), val(4, 5));
    }

    #[test]
    fn unknown_startup() {
        let mut b = bench(RegisterOptions::default().width(4).startup(Startup::Unknown));
        assert_eq!(b.out(Q), Value::unknown(BitWidth::new(4)));
    }

    #[test]
    fn wide_register_64_bits() {
        let mut b = bench(RegisterOptions::default().width(64));
        b.set(D, val(64, u64::MAX - 1));
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(64, u64::MAX - 1));
    }
}
