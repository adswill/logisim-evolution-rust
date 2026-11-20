//! Counters: 7493, 74161, 74163, 74190, 74192, 74193, 74390, 74393, 744017, 744060.

use super::*;
use logisim_core::{BitWidth, Value};

fn contains_error(v: Value) -> bool {
    v.bits().contains(&Value::ERROR)
}

/// Increment a `w`-bit word modulo `2^w`; E/X propagate to the whole word.
fn next_binary(cur: Value, w: u8) -> Value {
    let bw = BitWidth(w);
    if contains_error(cur) {
        Value::error(bw)
    } else if !cur.is_fully_defined() {
        Value::unknown(bw)
    } else {
        Value::known(bw, (cur.to_u64().unwrap() + 1) & bw.mask())
    }
}

fn toggle(b: Value) -> Value {
    match b {
        Value::TRUE => Value::FALSE,
        Value::FALSE => Value::TRUE,
        Value::ERROR => Value::ERROR,
        _ => Value::UNKNOWN,
    }
}

/// 7493: 4-bit ripple counter (÷2 stage CKA→QA, ÷8 stage CKB→QB..QD), falling edge, R0 reset.
pub fn chip_7493() -> Ttl {
    let spec = Spec::new("7493", 14, &[8, 9, 11, 12])
        .unused(&[4, 6, 7, 13])
        .power(5, 10);
    let mut q = Value::known(BitWidth(4), 0);
    let (mut ea, mut eb) = (Edge::new(), Edge::new());
    Ttl::new(spec, move |p| {
        let a = ea.falling(p.get(14));
        let b = eb.falling(p.get(1));
        if p.hi(2) && p.hi(3) {
            q = Value::known(BitWidth(4), 0);
        } else {
            let mut bits = q.bits();
            if a {
                bits[0] = !bits[0];
            }
            if b {
                let mut carry = Value::TRUE;
                for bit in bits.iter_mut().skip(1) {
                    let old = *bit;
                    *bit = old.xor(carry);
                    carry = old.and(carry);
                }
            }
            q = Value::from_bits(&bits);
        }
        for (pin, i) in [(12u8, 0u8), (9, 1), (8, 2), (11, 3)] {
            p.set(pin, q.bit(i), 4);
        }
    })
}

/// 74161 (async clear) / 74163 (sync clear) synchronous 4-bit counter.
fn counter_4(name: &'static str, sync_clear: bool) -> Ttl {
    let spec = Spec::new(name, 16, &[11, 12, 13, 14, 15]);
    let mut count: Option<u64> = Some(0);
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let trig = edge.rising(p.get(2));
        let clear = p.lo(1);
        let load = p.lo(9);
        let enable = p.hi(7) && p.hi(10);
        if clear && !sync_clear {
            count = Some(0);
        } else if trig {
            if clear {
                count = Some(0);
            } else if load {
                count = p.read(&[3, 4, 5, 6]);
            } else if enable {
                count = count.map(|c| (c + 1) & 15);
            }
        }
        match count {
            Some(c) => {
                p.write(&[14, 13, 12, 11], c, 1);
                let all = p.get(10).and(bit(c & 1 == 1)).and(bit(c & 2 != 0))
                    .and(bit(c & 4 != 0))
                    .and(bit(c & 8 != 0));
                p.set(15, all, 1);
            }
            None => p.write_unknown(&[14, 13, 12, 11, 15], 1),
        }
    })
}
pub fn chip_74161() -> Ttl {
    counter_4("74161", false)
}
pub fn chip_74163() -> Ttl {
    counter_4("74163", true)
}

// ---- 74190 ---------------------------------------------------------------------------------

const COUNT_UP: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 9, 4, 9, 0, 9, 0];
const COUNT_DOWN: [u8; 16] = [9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 3, 4, 5, 6];

/// Collects the set of results over all inputs consistent with partly unknown sources
/// (Logisim's `Choice`): bits on which results disagree become X (E if an error was involved).
struct Choice {
    width: u8,
    bits: Vec<Value>,
    conflict: Vec<bool>,
    saw_error: bool,
    any: bool,
}
impl Choice {
    fn new(width: u8) -> Choice {
        Choice {
            width,
            bits: vec![Value::FALSE; width as usize],
            conflict: vec![false; width as usize],
            saw_error: false,
            any: false,
        }
    }
    fn accept(&mut self, value: u32, error: bool) {
        self.saw_error |= error;
        for i in 0..self.width as usize {
            let b = bit((value >> i) & 1 == 1);
            if !self.any {
                self.bits[i] = b;
            } else if self.bits[i] != b {
                self.conflict[i] = true;
            }
        }
        self.any = true;
    }
    fn value(mut self) -> Value {
        if !self.any {
            return Value::unknown(BitWidth(self.width));
        }
        for i in 0..self.width as usize {
            if self.conflict[i] {
                self.bits[i] = if self.saw_error {
                    Value::ERROR
                } else {
                    Value::UNKNOWN
                };
            }
        }
        Value::from_bits(&self.bits)
    }
}

/// True if mask (bit i = source i is high) is consistent with the known sources.
fn accepts(sources: &[Value], mask: u32) -> bool {
    sources.iter().enumerate().all(|(i, s)| {
        let high = mask & (1 << i) != 0;
        !((*s == Value::TRUE && !high) || (*s == Value::FALSE && high))
    })
}

fn any_error(sources: &[Value]) -> bool {
    sources.contains(&Value::ERROR)
}

/// 74190: synchronous BCD up/down decade counter with asynchronous parallel load.
///
/// Pins: D0=15 D1=1 D2=10 D3=9, Q0=3 Q1=2 Q2=6 Q3=7, CE=4 (low = count), D/U=5 (high = down),
/// PL=11 (low = load), CP=14, TC=12, RC=13 (ripple clock).
pub fn chip_74190() -> Ttl {
    let spec = Spec::new("74190", 16, &[2, 3, 6, 7, 12, 13]);
    let mut stored = Value::known(BitWidth(4), 0);
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let clock = p.get(14);
        let triggered = edge.rising(clock);
        let (pl, ce, du) = (p.get(11), p.get(4), p.get(5));
        let d = [p.get(15), p.get(1), p.get(10), p.get(9)];
        let sources = [
            pl,
            ce,
            du,
            d[0],
            d[1],
            d[2],
            d[3],
            stored.bit(0),
            stored.bit(1),
            stored.bit(2),
            stored.bit(3),
        ];
        let mut ch = Choice::new(4);
        for mask in 0..(1u32 << 11) {
            if !accepts(&sources, mask) {
                continue;
            }
            let load_high = mask & 1 != 0;
            let counting = triggered && mask & 2 == 0;
            let down = mask & 4 != 0;
            let data = (mask >> 3) & 0xF;
            let current = ((mask >> 7) & 0xF) as usize;
            let next = if !load_high {
                data
            } else if counting {
                (if down { COUNT_DOWN } else { COUNT_UP })[current] as u32
            } else {
                current as u32
            };
            ch.accept(next, any_error(&sources));
        }
        stored = ch.value();
        // flags: bit0 = TC, bit1 = RC
        let sources = [
            ce,
            du,
            clock,
            stored.bit(0),
            stored.bit(1),
            stored.bit(2),
            stored.bit(3),
        ];
        let mut fl = Choice::new(2);
        for mask in 0..(1u32 << 7) {
            if !accepts(&sources, mask) {
                continue;
            }
            let enabled = mask & 1 == 0;
            let down = mask & 2 != 0;
            let clock_high = mask & 4 != 0;
            let code = (mask >> 3) & 0xF;
            let terminal = if down { code == 0 } else { code & 9 == 9 };
            let ripple_high = !(terminal && enabled && !clock_high);
            fl.accept(terminal as u32 | (ripple_high as u32) << 1, any_error(&sources));
        }
        let flags = fl.value();
        for (pin, i) in [(3u8, 0u8), (2, 1), (6, 2), (7, 3)] {
            p.set(pin, stored.bit(i), 1);
        }
        p.set(12, flags.bit(0), 1);
        p.set(13, flags.bit(1), 1);
    })
}

// ---- 74192 / 74193 -------------------------------------------------------------------------

/// Dual-clock up/down counter. `max` = 9 (74192 decade) or 15 (74193 binary).
fn updown(name: &'static str, max: i64) -> Ttl {
    let spec = Spec::new(name, 16, &[2, 3, 6, 7, 12, 13]);
    let mut counter: i64 = 0;
    let (mut carry, mut borrow) = (Value::TRUE, Value::TRUE);
    let (mut down_prev, mut up_prev) = (Value::TRUE, Value::TRUE);
    Ttl::new(spec, move |p| {
        let (down, up) = (p.get(4), p.get(5));
        let down_falling = down_prev == Value::TRUE && down == Value::FALSE;
        let down_rising = down_prev == Value::FALSE && down == Value::TRUE;
        let up_falling = up_prev == Value::TRUE && up == Value::FALSE;
        let up_rising = up_prev == Value::FALSE && up == Value::TRUE;
        let down_high = down_prev == Value::TRUE && down == Value::TRUE;
        let up_high = up_prev == Value::TRUE && up == Value::TRUE;
        let (mut c, mut b) = (Value::TRUE, Value::TRUE);
        if p.hi(14) {
            counter = 0;
        } else if p.lo(11) {
            let v = p.read(&[15, 1, 10, 9]).unwrap_or(0) as i64;
            counter = if v > max { 0 } else { v };
        } else if down_rising && up_high {
            counter -= 1;
            if counter < 0 {
                counter = max;
            }
        } else if up_rising && down_high {
            counter += 1;
            if counter > max {
                counter = 0;
            }
        } else if up_falling && down_high {
            if counter == max {
                c = Value::FALSE;
            }
        } else if down_falling && up_high {
            if counter == 0 {
                b = Value::FALSE;
            }
        } else {
            c = carry;
            b = borrow;
        }
        carry = c;
        borrow = b;
        down_prev = down;
        up_prev = up;
        p.write(&[3, 2, 6, 7], counter as u64, 4);
        p.set(12, carry, 4);
        p.set(13, borrow, 4);
    })
}
pub fn chip_74192() -> Ttl {
    updown("74192", 9)
}
pub fn chip_74193() -> Ttl {
    updown("74193", 15)
}

// ---- dual counters -------------------------------------------------------------------------

/// 74390: dual decade (÷2 and ÷5) counter, falling edges, MR active high.
pub fn chip_74390() -> Ttl {
    let spec = Spec::new("74390", 16, &[3, 5, 6, 7, 9, 10, 11, 13]);
    let mut q = [Value::known(BitWidth(4), 0); 2];
    let mut edges = [Edge::new(); 4];
    Ttl::new(spec, move |p| {
        // per half: (CP0, MR, Q0..Q3, CP1)
        let halves = [(1u8, 2u8, [3u8, 5, 6, 7], 4u8), (15, 14, [13, 11, 10, 9], 12)];
        for (h, &(cp0, mr, qs, cp1)) in halves.iter().enumerate() {
            let by2 = edges[h * 2].falling(p.get(cp0));
            let by5 = edges[h * 2 + 1].falling(p.get(cp1));
            if p.hi(mr) {
                q[h] = Value::known(BitWidth(4), 0);
            } else {
                let mut bits = q[h].bits();
                if by2 {
                    bits[0] = toggle(bits[0]);
                }
                if by5 {
                    advance_div5(&mut bits);
                }
                q[h] = Value::from_bits(&bits);
            }
            for (i, &pin) in qs.iter().enumerate() {
                p.set(pin, q[h].bit(i as u8), 4);
            }
        }
    })
}

fn advance_div5(bits: &mut [Value]) {
    let (q1, q2, q3) = (bits[1], bits[2], bits[3]);
    if [q1, q2, q3].contains(&Value::ERROR) {
        bits[1..4].fill(Value::ERROR);
        return;
    }
    if ![q1, q2, q3].iter().all(|b| *b == Value::TRUE || *b == Value::FALSE) {
        bits[1..4].fill(Value::UNKNOWN);
        return;
    }
    // Q1: J = !Q3, K = 1  → toggle when J, else reset
    let next_q1 = if q3 == Value::FALSE { toggle(q1) } else { Value::FALSE };
    let next_q3 = if q1 == Value::TRUE && q2 == Value::TRUE { toggle(q3) } else { Value::FALSE };
    let next_q2 = if q1 == Value::TRUE { toggle(q2) } else { q2 };
    bits[1] = next_q1;
    bits[2] = next_q2;
    bits[3] = next_q3;
}

/// 74393: dual 4-bit binary ripple counter, falling edge, MR active high.
pub fn chip_74393() -> Ttl {
    let spec = Spec::new("74393", 14, &[3, 4, 5, 6, 8, 9, 10, 11]);
    let mut q = [Value::known(BitWidth(4), 0); 2];
    let mut edges = [Edge::new(); 2];
    Ttl::new(spec, move |p| {
        let halves = [(1u8, 2u8, [3u8, 4, 5, 6]), (13, 12, [11, 10, 9, 8])];
        for (h, &(cp, mr, qs)) in halves.iter().enumerate() {
            let clocked = edges[h].falling(p.get(cp));
            if p.hi(mr) {
                q[h] = Value::known(BitWidth(4), 0);
            } else if clocked {
                q[h] = next_binary(q[h], 4);
            }
            for (i, &pin) in qs.iter().enumerate() {
                p.set(pin, q[h].bit(i as u8), 4);
            }
        }
    })
}

/// 744017: decade counter with 10 decoded outputs. CP0 rising (CP1 low) or CP1 falling (CP0
/// high) advances; MR active high; Q5-9 carry is high for counts 0-4.
pub fn chip_744017() -> Ttl {
    const Q: [u8; 10] = [3, 2, 4, 7, 10, 1, 5, 6, 9, 11];
    let spec = Spec::new("744017", 16, &[1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12]);
    let mut count = Value::known(BitWidth(4), 0);
    let (mut e0, mut e1) = (Edge::new(), Edge::new());
    Ttl::new(spec, move |p| {
        let (cp0, cp1) = (p.get(14), p.get(13));
        let r0 = e0.rising(cp0);
        let f1 = e1.falling(cp1);
        let next = |cur: Value| {
            if contains_error(cur) {
                Value::error(BitWidth(4))
            } else if !cur.is_fully_defined() || cur.to_u64().unwrap() > 9 {
                Value::unknown(BitWidth(4))
            } else {
                Value::known(BitWidth(4), (cur.to_u64().unwrap() + 1) % 10)
            }
        };
        if p.hi(15) {
            count = Value::known(BitWidth(4), 0);
        } else if (r0 && cp1 == Value::FALSE) || (f1 && cp0 == Value::TRUE) {
            count = next(count);
        }
        if contains_error(count) || !count.is_fully_defined() || count.to_u64().unwrap() > 9 {
            let b = if contains_error(count) { Value::ERROR } else { Value::UNKNOWN };
            for &pin in Q.iter().chain([12u8].iter()) {
                p.set(pin, b, 4);
            }
        } else {
            let step = count.to_u64().unwrap() as usize;
            for (i, &pin) in Q.iter().enumerate() {
                p.set_bool(pin, i == step, 4);
            }
            p.set_bool(12, step < 5, 4);
        }
    })
}

/// 744060: 14-stage binary ripple counter (only Q4-Q10, Q12-Q14 are pinned out).
pub fn chip_744060() -> Ttl {
    // (pin, stage bit)
    const OUT: [(u8, u8); 10] = [
        (1, 11),
        (2, 12),
        (3, 13),
        (4, 5),
        (5, 4),
        (6, 6),
        (7, 3),
        (13, 8),
        (14, 7),
        (15, 9),
    ];
    let spec = Spec::new("744060", 16, &[1, 2, 3, 4, 5, 6, 7, 13, 14, 15]).unused(&[9, 10]);
    let mut count = Value::known(BitWidth(14), 0);
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let clocked = edge.falling(p.get(11));
        if p.hi(12) {
            count = Value::known(BitWidth(14), 0);
        } else if clocked {
            count = next_binary(count, 14);
        }
        for &(pin, b) in OUT.iter() {
            p.set(pin, count.bit(b), 4);
        }
    })
}

pub fn parts() -> Vec<PartDesc> {
    chips! {
        "7493" => chip_7493, "74161" => chip_74161, "74163" => chip_74163,
        "74190" => chip_74190, "74192" => chip_74192, "74193" => chip_74193,
        "74390" => chip_74390, "74393" => chip_74393, "744017" => chip_744017,
        "744060" => chip_744060,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::Rig;
    use super::*;

    #[test]
    fn ripple_7493() {
        let mut r = Rig::new(chip_7493());
        // connect QA to CKB externally by hand: count with both clocks
        for n in 1..=5u64 {
            // QA toggles on CKA falling edge
            r.put(14, true);
            r.put(14, false);
            let qa = r.hi(12);
            assert_eq!(qa, n % 2 == 1);
            // when QA falls, clock B manually (as the datasheet wiring would)
            if !qa {
                r.put(1, true);
                r.put(1, false);
            }
        }
        assert_eq!(r.read_n(&[12, 9, 8, 11]), Some(5));
        r.put(2, true);
        r.put(3, true);
        assert_eq!(r.read_n(&[12, 9, 8, 11]), Some(0));
    }

    #[test]
    fn counter_74161_counts_loads_clears() {
        let mut r = Rig::new(chip_74161());
        r.put(1, true);
        r.put(9, true);
        r.put(7, true);
        r.put(10, true);
        for _ in 0..3 {
            r.pulse(2);
        }
        assert_eq!(r.read_n(&[14, 13, 12, 11]), Some(3));
        r.put_n(&[3, 4, 5, 6], 14);
        r.put(9, false);
        r.pulse(2);
        r.put(9, true);
        assert_eq!(r.read_n(&[14, 13, 12, 11]), Some(14));
        assert!(!r.hi(15));
        r.pulse(2);
        assert!(r.hi(15)); // 15 with EnT high → RC0
        r.pulse(2);
        assert_eq!(r.read_n(&[14, 13, 12, 11]), Some(0)); // wraps
        r.put(7, false); // hold
        r.pulse(2);
        assert_eq!(r.read_n(&[14, 13, 12, 11]), Some(0));
        r.put(7, true);
        r.pulse(2);
        r.put(1, false); // async clear
        assert_eq!(r.read_n(&[14, 13, 12, 11]), Some(0));
    }

    #[test]
    fn counter_74163_sync_clear() {
        let mut r = Rig::new(chip_74163());
        r.put(1, true);
        r.put(9, true);
        r.put(7, true);
        r.put(10, true);
        r.pulse(2);
        r.pulse(2);
        assert_eq!(r.read_n(&[14, 13, 12, 11]), Some(2));
        r.put(1, false);
        assert_eq!(r.read_n(&[14, 13, 12, 11]), Some(2)); // not yet
        r.pulse(2);
        assert_eq!(r.read_n(&[14, 13, 12, 11]), Some(0));
    }

    #[test]
    fn up_down_74192_74193() {
        let q = [3u8, 2, 6, 7];
        let mut r = Rig::new(chip_74192());
        r.put(4, true);
        r.put(5, true);
        r.put(11, true);
        for _ in 0..3 {
            r.put(5, false);
            r.put(5, true);
        }
        assert_eq!(r.read_n(&q), Some(3));
        r.put(4, false);
        r.put(4, true);
        assert_eq!(r.read_n(&q), Some(2));
        r.put_n(&[15, 1, 10, 9], 9);
        r.put(11, false);
        r.put(11, true);
        assert_eq!(r.read_n(&q), Some(9));
        r.put(5, false);
        r.put(5, true);
        assert_eq!(r.read_n(&q), Some(0)); // decade wrap
        r.put(14, true);
        r.put(14, false);
        let mut r = Rig::new(chip_74193());
        r.put(4, true);
        r.put(5, true);
        r.put(11, true);
        r.put(4, false);
        assert!(!r.hi(13)); // borrow goes low on the falling edge at 0
        r.put(4, true);
        assert_eq!(r.read_n(&q), Some(15)); // binary wrap on down
        assert!(r.hi(13));
    }

    #[test]
    fn decade_74190() {
        let q = [3u8, 2, 6, 7];
        let mut r = Rig::new(chip_74190());
        r.put(11, true); // PL high
        r.put(4, false); // CE low = count
        r.put(5, false); // up
        for n in 1..=10u64 {
            r.pulse(14);
            assert_eq!(r.read_n(&q), Some(n % 10));
        }
        r.put_n(&[15, 1, 10, 9], 5);
        r.put(11, false);
        assert_eq!(r.read_n(&q), Some(5)); // async load
        r.put(11, true);
        r.put(5, true); // down
        r.pulse(14);
        assert_eq!(r.read_n(&q), Some(4));
        r.put(4, true); // disabled
        r.pulse(14);
        assert_eq!(r.read_n(&q), Some(4));
    }

    #[test]
    fn dual_counters() {
        let mut r = Rig::new(chip_74393());
        for n in 1..=18u64 {
            r.pulse(1);
            r.pulse(1); // no-op shape; each pulse = one falling edge
            assert_eq!(r.read_n(&[3, 4, 5, 6]), Some((2 * n) & 15));
        }
        r.put(2, true);
        assert_eq!(r.read_n(&[3, 4, 5, 6]), Some(0));

        let mut r = Rig::new(chip_74390());
        for n in 1..=6u64 {
            r.pulse(4); // ÷5 section
            let v = r.read_n(&[5, 6, 7]).unwrap();
            let expect = [1, 2, 3, 4, 0, 1][(n - 1) as usize];
            assert_eq!(v, expect);
        }
        r.pulse(1);
        assert!(r.hi(3));
        r.put(2, true);
        assert_eq!(r.read_n(&[3, 5, 6, 7]), Some(0));
    }

    #[test]
    fn johnson_744017_and_744060() {
        const Q: [u8; 10] = [3, 2, 4, 7, 10, 1, 5, 6, 9, 11];
        let mut r = Rig::new(chip_744017());
        assert!(r.hi(3) && r.hi(12));
        for step in 1..=10usize {
            r.pulse(14);
            let s = step % 10;
            for (i, &pin) in Q.iter().enumerate() {
                assert_eq!(r.hi(pin), i == s, "step {step} out {i}");
            }
            assert_eq!(r.hi(12), s < 5);
        }
        r.pulse(14);
        r.put(15, true);
        assert!(r.hi(3));

        let mut r = Rig::new(chip_744060());
        for _ in 0..8 {
            r.pulse(11);
        }
        // 8 clocks: only stage bit 3 (Q4, pin 7) is high
        assert!(r.hi(7));
        assert!(!r.hi(5) && !r.hi(4));
    }
}
