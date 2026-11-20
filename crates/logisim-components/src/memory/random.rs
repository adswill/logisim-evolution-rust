//! Random number generator (Java `Random` + `Xoshiro256PlusPlus`).
//!
//! Port order (east-facing, classic layout, translated so the output is at (0,0)):
//! `0` Q (0,0), `width` bits · `1` clock (-30,20) · `2` next (-40,10) · `3` reset (-10,20).
//! Each enabled clock trigger advances the sequence; reset restarts it from the seed.
//! A seed of 0 means "pick a fresh time-based seed" (at creation and on each reset pulse).

use logisim_core::{BitWidth, Component, Port, PortKind, PropagateCtx, Value};

use super::{ClockState, Trigger, SHIFT_DELAY};

pub const Q: usize = 0;
pub const CLOCK: usize = 1;
pub const NEXT: usize = 2;
pub const RESET: usize = 3;

const SPLIT_MIX_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// xoshiro256++ seeded through SplitMix64, exactly as the Java source.
#[derive(Debug, Clone)]
pub struct Xoshiro256PlusPlus {
    state: [u64; 4],
}

impl Xoshiro256PlusPlus {
    pub fn new(seed: u64) -> Self {
        let mut state = [0u64; 4];
        let mut sm = seed;
        for s in &mut state {
            sm = sm.wrapping_add(SPLIT_MIX_GAMMA);
            let mut v = sm;
            v = (v ^ (v >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            v = (v ^ (v >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            *s = v ^ (v >> 31);
        }
        Xoshiro256PlusPlus { state }
    }
    pub fn current(&self) -> u64 {
        self.state[0]
            .wrapping_add(self.state[3])
            .rotate_left(23)
            .wrapping_add(self.state[0])
    }
    pub fn step(&mut self) {
        let s = &mut self.state;
        let shifted = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= shifted;
        s[3] = s[3].rotate_left(45);
    }
}

#[derive(Debug, Clone)]
pub struct RandomOptions {
    pub width: BitWidth,
    /// 0 = time-based.
    pub seed: u32,
    /// Edge trigger only.
    pub trigger: Trigger,
}

impl Default for RandomOptions {
    fn default() -> Self {
        RandomOptions {
            width: BitWidth::new(8),
            seed: 0,
            trigger: Trigger::Rising,
        }
    }
}

impl RandomOptions {
    pub fn width(mut self, w: u8) -> Self {
        assert!((1..=64).contains(&w), "width must be 1..=64");
        self.width = BitWidth::new(w);
        self
    }
    pub fn seed(mut self, s: u32) -> Self {
        self.seed = s;
        self
    }
    /// # Panics
    /// For level triggers, which the random generator does not support.
    pub fn trigger(mut self, t: Trigger) -> Self {
        assert!(t.is_edge(), "random generators only support edge triggers");
        self.trigger = t;
        self
    }
}

pub struct Random {
    opts: RandomOptions,
    clock: ClockState,
    reset_seed: u64,
    generator: Xoshiro256PlusPlus,
    value: u64,
    old_reset: Value,
}

impl Random {
    pub fn new(opts: RandomOptions) -> Self {
        let mut r = Random {
            reset_seed: 0,
            generator: Xoshiro256PlusPlus::new(0),
            value: 0,
            old_reset: Value::UNKNOWN,
            clock: ClockState::default(),
            opts,
        };
        r.reset_seed = r.pick_seed();
        r.reset();
        r
    }

    fn pick_seed(&self) -> u64 {
        let seed = u64::from(self.opts.seed);
        if seed != 0 {
            return seed;
        }
        let mut s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_millis() as u64);
        if s == self.reset_seed {
            s = s.wrapping_add(SPLIT_MIX_GAMMA);
        }
        s
    }

    fn reset(&mut self) {
        self.generator = Xoshiro256PlusPlus::new(self.reset_seed);
        self.value = self.generator.current();
    }

    /// The raw 64-bit value (the output is its low `width` bits).
    pub fn raw_value(&self) -> u64 {
        self.value
    }
}

impl Component for Random {
    fn name(&self) -> &str {
        "Random"
    }
    fn ports(&self) -> Vec<Port> {
        let one = BitWidth::ONE;
        vec![
            Port::new(0, 0, PortKind::Output, self.opts.width),
            Port::new(-30, 20, PortKind::Input, one),
            Port::new(-40, 10, PortKind::Input, one),
            Port::new(-10, 20, PortKind::Input, one),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let triggered = self.clock.update(ctx.port_value(CLOCK), self.opts.trigger);
        let reset = ctx.port_value(RESET);
        if self.old_reset == Value::FALSE && reset == Value::TRUE {
            self.reset_seed = self.pick_seed();
        }
        self.old_reset = reset;
        if reset == Value::TRUE {
            self.reset();
        } else if triggered && ctx.port_value(NEXT) != Value::FALSE {
            self.generator.step();
            self.value = self.generator.current();
        }
        ctx.set_port(Q, Value::known(self.opts.width, self.value), SHIFT_DELAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::testutil::Bench;

    const T: Value = Value::TRUE;
    const F: Value = Value::FALSE;

    #[test]
    fn xoshiro_matches_reference_sequence() {
        // computed independently from the algorithm in the Java source (seed = 1)
        let mut g = Xoshiro256PlusPlus::new(1);
        let want = [
            0xcfc5_d07f_6f03_c29b_u64,
            0xbf42_4132_963f_e08d,
            0x19a3_7d57_57aa_f520,
            0xbf08_119f_05cd_56d6,
        ];
        for w in want {
            assert_eq!(g.current(), w);
            g.step();
        }
    }

    fn bench(seed: u32, width: u8) -> Bench {
        let mut b = Bench::new(Box::new(Random::new(
            RandomOptions::default().seed(seed).width(width),
        )));
        b.drive(CLOCK, F);
        b.drive(NEXT, T);
        b.drive(RESET, F);
        b.start();
        b
    }

    fn val(w: u8, v: u64) -> Value {
        Value::known(BitWidth::new(w), v)
    }

    #[test]
    fn outputs_seeded_sequence_one_step_per_clock() {
        let mut b = bench(1, 64);
        assert_eq!(b.out(Q), val(64, 0xcfc5_d07f_6f03_c29b));
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(64, 0xbf42_4132_963f_e08d));
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(64, 0x19a3_7d57_57aa_f520));
    }

    #[test]
    fn narrow_width_takes_low_bits() {
        let mut b = bench(1, 8);
        assert_eq!(b.out(Q), val(8, 0x9b));
    }

    #[test]
    fn reset_restarts_and_next_low_pauses() {
        let mut b = bench(1, 64);
        b.set(NEXT, F);
        b.pulse(CLOCK);
        assert_eq!(b.out(Q), val(64, 0xcfc5_d07f_6f03_c29b), "paused");
        b.set(NEXT, T);
        b.pulse(CLOCK);
        b.pulse(CLOCK);
        b.set(RESET, T);
        assert_eq!(b.out(Q), val(64, 0xcfc5_d07f_6f03_c29b));
        b.set(CLOCK, T);
        assert_eq!(b.out(Q), val(64, 0xcfc5_d07f_6f03_c29b), "held in reset");
    }

    #[test]
    fn same_seed_is_reproducible_and_zero_seed_runs() {
        let (mut a, mut c) = (bench(77, 16), bench(77, 16));
        for _ in 0..5 {
            a.pulse(CLOCK);
            c.pulse(CLOCK);
        }
        assert_eq!(a.out(Q), c.out(Q));
        let mut z = bench(0, 16);
        assert!(z.out(Q).is_fully_defined());
    }
}
