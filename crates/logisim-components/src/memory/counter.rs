//! Counter (Java `Counter`).
//!
//! Port order (east-facing, classic layout): `0` Q (0,0) · `1` data in (-30,0) · `2` clock (-20,20) ·
//! `3` clear (-10,20) · `4` load (-30,-10) · `5` up/down (-20,-20) · `6` enable (-30,10) ·
//! `7` carry (0,10). Up/down counts up unless exactly 0; enable is active unless exactly 0;
//! load takes priority over counting, clear over everything.
//!
//! One deliberate difference from the Java source: when the counter holds an unknown/error value
//! and the clock does not fire, the Java code outputs all-ones; here the unknown value is kept.

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

use super::register::initial;
use super::{long_value, parse_u64, ClockState, Startup, Trigger, REGISTER_DELAY, STARTUPS, TRIGGERS};

pub const Q: usize = 0;
pub const DATA: usize = 1;
pub const CLOCK: usize = 2;
pub const CLEAR: usize = 3;
pub const LOAD: usize = 4;
pub const UP_DOWN: usize = 5;
pub const ENABLE: usize = 6;
pub const CARRY: usize = 7;

/// What happens when counting past the goal (max when counting up, 0 when counting down).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnGoal {
    /// Wrap around to the other end.
    #[default]
    Wrap,
    /// Stay at the goal.
    Stay,
    /// Keep counting (the value wraps at the bit width).
    Continue,
    /// Load the data input.
    Load,
}

const ON_GOALS: [(&str, OnGoal); 4] = [
    ("wrap", OnGoal::Wrap),
    ("stay", OnGoal::Stay),
    ("continue", OnGoal::Continue),
    ("load", OnGoal::Load),
];

#[derive(Debug, Clone)]
pub struct CounterOptions {
    pub width: BitWidth,
    /// Counting limit; always within `width`.
    pub max: u64,
    pub on_goal: OnGoal,
    /// Edge trigger only (rising or falling).
    pub trigger: Trigger,
    pub startup: Startup,
}

impl Default for CounterOptions {
    fn default() -> Self {
        CounterOptions {
            width: BitWidth::new(8),
            max: 0xFF,
            on_goal: OnGoal::Wrap,
            trigger: Trigger::Rising,
            startup: Startup::Zero,
        }
    }
}

impl CounterOptions {
    /// Sets the width. As in Logisim, `max` follows: it is masked when the width shrinks and
    /// becomes the all-ones value when the width grows.
    pub fn width(mut self, w: u8) -> Self {
        assert!((1..=64).contains(&w), "counter width must be 1..=64");
        let new = BitWidth::new(w);
        self.max = if w < self.width.get() {
            new.mask() & self.max
        } else {
            new.mask()
        };
        self.width = new;
        self
    }
    /// Sets the maximum value (masked to the width).
    pub fn max(mut self, m: u64) -> Self {
        self.max = m & self.width.mask();
        self
    }
    pub fn on_goal(mut self, g: OnGoal) -> Self {
        self.on_goal = g;
        self
    }
    /// # Panics
    /// For level triggers, which counters do not support.
    pub fn trigger(mut self, t: Trigger) -> Self {
        assert!(t.is_edge(), "counters only support edge triggers");
        self.trigger = t;
        self
    }
    pub fn startup(mut self, s: Startup) -> Self {
        self.startup = s;
        self
    }
}

pub struct Counter {
    opts: CounterOptions,
    clock: ClockState,
    value: Value,
}

impl Counter {
    pub fn new(opts: CounterOptions) -> Self {
        let value = initial(opts.width, opts.startup);
        Counter {
            opts,
            clock: ClockState::default(),
            value,
        }
    }
    pub fn value(&self) -> Value {
        self.value
    }
}

impl Component for Counter {
    fn name(&self) -> &str {
        "Counter"
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("width", "Data bits", 1, 64, 8),
            OptionSpec::string("max", "Maximum value (decimal)", "255"),
            OptionSpec::choice(
                "on_goal",
                "When the goal is reached",
                &["wrap", "stay", "continue", "load"],
                "wrap",
            ),
            OptionSpec::choice("trigger", "Trigger", &["rising", "falling"], "rising"),
            OptionSpec::choice("startup", "Value at power-on", &["zero", "unknown"], "zero"),
        ]
    }
    fn options(&self) -> OptionMap {
        let o = &self.opts;
        OptionMap::new()
            .with("width", o.width.get())
            .with("max", o.max.to_string())
            .with("on_goal", OptionMap::name_of(o.on_goal, &ON_GOALS))
            .with("trigger", OptionMap::name_of(o.trigger, &TRIGGERS))
            .with("startup", OptionMap::name_of(o.startup, &STARTUPS))
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width", "max", "on_goal", "trigger", "startup"])?;
        let mut o = self.opts.clone();
        if let Some(v) = m.int("width", 1, 64)? {
            o = o.width(v as u8);
        }
        // `max` is an unsigned 64-bit number, saved as text so values above i64::MAX survive
        if let Some(s) = m.string("max")? {
            o = o.max(parse_u64(s)?);
        }
        o.on_goal = m.choice("on_goal", &ON_GOALS)?.unwrap_or(o.on_goal);
        let trigger = m.choice("trigger", &TRIGGERS)?.unwrap_or(o.trigger);
        if !trigger.is_edge() {
            return Err("counters only support rising or falling triggers".into());
        }
        o.trigger = trigger;
        o.startup = m.choice("startup", &STARTUPS)?.unwrap_or(o.startup);
        *self = Counter::new(o);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let (w, one) = (self.opts.width, BitWidth::ONE);
        vec![
            Port::new(0, 0, PortKind::Output, w),
            Port::new(-30, 0, PortKind::Input, w),
            Port::new(-20, 20, PortKind::Input, one),
            Port::new(-10, 20, PortKind::Input, one),
            Port::new(-30, -10, PortKind::Input, one),
            Port::new(-20, -20, PortKind::Input, one),
            Port::new(-30, 10, PortKind::Input, one),
            Port::new(0, 10, PortKind::Output, one),
        ]
    }

    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let width = self.opts.width;
        let max = i128::from(self.opts.max);
        let triggered = self.clock.update(ctx.port_value(CLOCK), self.opts.trigger);

        let carry;
        if ctx.port_value(CLEAR) == Value::TRUE {
            self.value = Value::known(width, 0);
            carry = false;
        } else {
            let ld = ctx.port_value(LOAD) == Value::TRUE;
            let en = ctx.port_value(ENABLE) != Value::FALSE;
            let up = ctx.port_value(UP_DOWN) != Value::FALSE;
            let load_value = {
                let v = i128::from(long_value(ctx.port_value(DATA)));
                if v > max {
                    v & max
                } else {
                    v
                }
            };
            // `None` = the counter holds an undefined value and was clocked: becomes an error
            let new: Option<i128> = if !triggered {
                self.value.to_u64().map(i128::from)
            } else if ld {
                Some(load_value)
            } else if !self.value.is_fully_defined() {
                None
            } else if en {
                let old = i128::from(long_value(self.value));
                let goal = if up { max } else { 0 };
                let step = if up { old + 1 } else { old - 1 };
                Some(if old == goal {
                    match self.opts.on_goal {
                        OnGoal::Wrap => {
                            if up {
                                0
                            } else {
                                max
                            }
                        }
                        OnGoal::Stay => old,
                        OnGoal::Load => load_value,
                        OnGoal::Continue => step,
                    }
                } else {
                    step
                })
            } else {
                Some(i128::from(long_value(self.value)))
            };
            match new {
                Some(n) => {
                    // `as u64` wraps like Java's `BigInteger.longValue()`; `known` masks to width
                    self.value = Value::known(width, n as u64);
                    carry = n == if up { max } else { 0 };
                }
                None => {
                    if triggered {
                        self.value = Value::error(width);
                    }
                    carry = false;
                }
            }
        }
        ctx.set_port(Q, self.value, REGISTER_DELAY);
        ctx.set_port(
            CARRY,
            if carry { Value::TRUE } else { Value::FALSE },
            REGISTER_DELAY,
        );
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

    fn bench(opts: CounterOptions) -> Bench {
        let w = opts.width.get();
        let mut b = Bench::new(Box::new(Counter::new(opts)));
        b.drive(DATA, val(w, 0));
        b.drive(CLOCK, F);
        b.drive(CLEAR, F);
        b.drive(LOAD, F);
        b.drive(UP_DOWN, T);
        b.drive(ENABLE, T);
        b.start();
        b
    }

    #[test]
    fn counts_up_and_wraps() {
        let mut b = bench(CounterOptions::default().width(2));
        let mut seen = vec![];
        for _ in 0..5 {
            b.pulse(CLOCK);
            seen.push(b.out(Q).to_u64().unwrap());
        }
        assert_eq!(seen, [1, 2, 3, 0, 1]);
    }

    #[test]
    fn carry_flags_the_goal() {
        let mut b = bench(CounterOptions::default().width(2));
        assert_eq!(b.out(CARRY), F);
        for _ in 0..3 {
            b.pulse(CLOCK);
        }
        assert_eq!(b.out(Q), val(2, 3));
        assert_eq!(b.out(CARRY), T, "carry while at max");
        b.pulse(CLOCK);
        assert_eq!(b.out(CARRY), F);
    }

    #[test]
    fn counts_down_with_borrow() {
        let mut b = bench(CounterOptions::default().width(3));
        b.set(UP_DOWN, F);
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(3, 7), "wraps down to max");
        assert_eq!(b.out(CARRY), F);
        b.set(DATA, val(3, 1));
        b.set(LOAD, T);
        b.pulse(CLOCK);
        b.set(LOAD, F);
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(3, 0));
        assert_eq!(b.out(CARRY), T, "carry at 0 when counting down");
    }

    #[test]
    fn custom_max_stay_and_load() {
        let mut b = bench(CounterOptions::default().width(4).max(9).on_goal(OnGoal::Stay));
        for _ in 0..12 {
            b.pulse(CLOCK);
        }
        assert_eq!(b.out(Q), val(4, 9));

        let mut b = bench(CounterOptions::default().width(4).max(9).on_goal(OnGoal::Wrap));
        for _ in 0..10 {
            b.pulse(CLOCK);
        }
        assert_eq!(b.out(Q), val(4, 0), "decade counter wraps 9 -> 0");

        let mut b = bench(CounterOptions::default().width(4).max(6).on_goal(OnGoal::Load));
        b.set(DATA, val(4, 5));
        for _ in 0..7 {
            b.pulse(CLOCK);
        }
        assert_eq!(b.out(Q), val(4, 5));
    }

    #[test]
    fn continue_runs_past_max_until_the_width_wraps() {
        let mut b = bench(CounterOptions::default().width(3).max(2).on_goal(OnGoal::Continue));
        let mut seen = vec![];
        for _ in 0..3 {
            b.pulse(CLOCK);
            seen.push(b.out(Q).to_u64().unwrap());
        }
        assert_eq!(seen, [1, 2, 3]);
    }

    #[test]
    fn load_clear_and_enable() {
        let mut b = bench(CounterOptions::default());
        b.set(DATA, val(8, 0x42));
        b.set(LOAD, T);
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(8, 0x42));
        b.set(LOAD, F);
        b.set(ENABLE, F);
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(8, 0x42), "disabled counter holds");
        b.set(ENABLE, T);
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(8, 0x43));
        b.set(CLEAR, T);
        assert_eq!(b.out(Q), val(8, 0));
    }

    #[test]
    fn load_value_above_max_is_masked_by_max() {
        let mut b = bench(CounterOptions::default().width(4).max(9));
        b.set(DATA, val(4, 15));
        b.set(LOAD, T);
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(4, 15 & 9));
    }

    #[test]
    fn falling_edge_trigger() {
        let mut b = bench(CounterOptions::default().trigger(Trigger::Falling));
        b.set(CLOCK, T);
        assert_eq!(b.out(Q), val(8, 0));
        b.set(CLOCK, F);
        assert_eq!(b.out(Q), val(8, 1));
    }

    #[test]
    fn unknown_value_becomes_error_when_clocked() {
        let mut b = bench(CounterOptions::default().width(4).startup(Startup::Unknown));
        assert_eq!(b.out(Q), Value::unknown(BitWidth::new(4)));
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), Value::error(BitWidth::new(4)));
        b.set(DATA, val(4, 3));
        b.set(LOAD, T);
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(4, 3), "load recovers");
    }

    #[test]
    fn width_setter_adjusts_max_like_logisim() {
        let o = CounterOptions::default().width(4);
        assert_eq!(o.max, 0xF);
        let o = o.max(9).width(2);
        assert_eq!(o.max, 1);
        assert_eq!(CounterOptions::default().width(64).max, u64::MAX);
    }
}
