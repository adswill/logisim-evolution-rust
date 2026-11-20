//! Registers and shift registers: 74173, 74175, 74273, 74374, 74377 (+ shift registers).

use super::*;
use logisim_core::Value;

/// 74173: 4-bit D register with tri-state outputs. CLR (15) async high; loads on rising CLK (7)
/// when both nG1 (9) and nG2 (10) are low; outputs enabled when nM (1) and nN (2) are low.
pub fn chip_74173() -> Ttl {
    const D: [u8; 4] = [14, 13, 12, 11];
    const Q: [u8; 4] = [3, 4, 5, 6];
    let spec = Spec::new("74173", 16, &Q);
    let mut state = [Value::FALSE; 4];
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let triggered = edge.rising(p.get(7));
        if p.hi(15) {
            state = [Value::FALSE; 4];
        } else if p.lo(9) && p.lo(10) && triggered {
            for (s, &d) in state.iter_mut().zip(D.iter()) {
                *s = p.get(d);
            }
        }
        let on = p.lo(1) && p.lo(2);
        for (i, &q) in Q.iter().enumerate() {
            p.set(q, if on { state[i] } else { Value::UNKNOWN }, 1);
        }
    })
}

/// 74175: quad D flip-flop with complementary outputs, async active-low clear, rising edge.
pub fn chip_74175() -> Ttl {
    let spec = Spec::new("74175", 16, &[2, 3, 6, 7, 10, 11, 14, 15]);
    let mut state = [Value::FALSE; 4];
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let triggered = edge.rising(p.get(9));
        if p.lo(1) {
            state = [Value::FALSE; 4];
        } else if triggered {
            for (s, d) in state.iter_mut().zip([4u8, 5, 12, 13]) {
                *s = p.get(d);
            }
        }
        for (i, (q, nq)) in [(2u8, 3u8), (7, 6), (10, 11), (15, 14)].into_iter().enumerate() {
            p.set(q, state[i], 8);
            p.set(nq, !state[i], 8);
        }
    })
}

const OCT_D: [u8; 8] = [3, 4, 7, 8, 13, 14, 17, 18];
const OCT_Q: [u8; 8] = [2, 5, 6, 9, 12, 15, 16, 19];

/// 74273: octal D flip-flop with async active-low clear (pin 1), clock on pin 11.
pub fn chip_74273() -> Ttl {
    let spec = Spec::new("74273", 20, &OCT_Q);
    let mut state = [Value::FALSE; 8];
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let triggered = edge.rising(p.get(11));
        if p.lo(1) {
            state = [Value::FALSE; 8];
        } else if triggered {
            for (s, &d) in state.iter_mut().zip(OCT_D.iter()) {
                *s = p.get(d);
            }
        }
        for (i, &q) in OCT_Q.iter().enumerate() {
            p.set(q, state[i], 8);
        }
    })
}

/// 74377: octal D flip-flop with active-low clock enable (pin 1), clock on pin 11.
pub fn chip_74377() -> Ttl {
    let spec = Spec::new("74377", 20, &OCT_Q);
    let mut state = [Value::FALSE; 8];
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let triggered = edge.rising(p.get(11));
        if triggered && p.lo(1) {
            for (s, &d) in state.iter_mut().zip(OCT_D.iter()) {
                *s = p.get(d);
            }
        }
        for (i, &q) in OCT_Q.iter().enumerate() {
            p.set(q, state[i], 8);
        }
    })
}

/// 74374: octal D flip-flop with tri-state outputs (nOE pin 1, active low), clock on pin 11.
pub fn chip_74374() -> Ttl {
    let spec = Spec::new("74374", 20, &OCT_Q);
    let mut state = [Value::FALSE; 8];
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        if edge.rising(p.get(11)) {
            for (s, &d) in state.iter_mut().zip(OCT_D.iter()) {
                *s = p.get(d);
            }
        }
        let on = p.lo(1);
        for (i, &q) in OCT_Q.iter().enumerate() {
            p.set(q, if on { state[i] } else { Value::UNKNOWN }, 1);
        }
    })
}

// ---- shift registers -------------------------------------------------------------------------
//
// State convention (as Logisim's `ShiftRegisterData`): index 0 is the output end; shifting "down"
// moves everything towards index 0 and inserts at the top index.

fn push_down(v: &mut [Value], x: Value) {
    v.rotate_left(1);
    let n = v.len();
    v[n - 1] = x;
}
fn push_up(v: &mut [Value], x: Value) {
    v.rotate_right(1);
    v[0] = x;
}

/// 74164: 8-bit serial-in parallel-out shift register, async clear (pin 9, active low).
/// Serial input = A AND B (pins 1, 2); clock pin 8; QA..QH = 3,4,5,6,10,11,12,13.
pub fn chip_74164() -> Ttl {
    const Q: [u8; 8] = [3, 4, 5, 6, 10, 11, 12, 13];
    let spec = Spec::new("74164", 14, &Q);
    let mut s = [Value::FALSE; 8];
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let triggered = edge.rising(p.get(8));
        if p.lo(9) {
            s = [Value::FALSE; 8];
        } else if triggered {
            push_up(&mut s, bit(p.hi(1) && p.hi(2)));
        }
        for (i, &q) in Q.iter().enumerate() {
            p.set(q, s[i], 4);
        }
    })
}

/// 74165: 8-bit parallel-load shift register. SH/LD = 1 (low loads, asynchronous), CLK = 2,
/// CLK INH = 15, serial in = 10, P0..P7 = 11,12,13,14,3,4,5,6, QH = 9, /QH = 7.
pub fn chip_74165() -> Ttl {
    // index 0 = H (output end)
    const P: [u8; 8] = [6, 5, 4, 3, 14, 13, 12, 11];
    let spec = Spec::new("74165", 16, &[7, 9]);
    let mut s = [Value::FALSE; 8];
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let triggered = edge.rising(p.get(2));
        if p.lo(1) {
            for (x, &pin) in s.iter_mut().zip(P.iter()) {
                *x = p.get(pin);
            }
        } else if triggered && p.lo(15) {
            push_down(&mut s, p.get(10));
        }
        p.set(7, !s[0], 4);
        p.set(9, s[0], 4);
    })
}

/// 74166: 8-bit parallel-load shift register with synchronous load and async clear.
/// SER = 1, P0..P3 = 2..5, CLK INH = 6, CLK = 7, CLR = 9, P4..P6 = 10..12, QH = 13, P7 = 14,
/// SH/LD = 15 (low loads).
pub fn chip_74166() -> Ttl {
    const P: [u8; 8] = [14, 12, 11, 10, 5, 4, 3, 2];
    let spec = Spec::new("74166", 16, &[13]);
    let mut s = [Value::FALSE; 8];
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let triggered = edge.rising(p.get(7));
        if p.lo(9) {
            s = [Value::FALSE; 8];
        } else if triggered && p.get(6) != Value::TRUE {
            match p.get(15) {
                Value::FALSE => {
                    for (x, &pin) in s.iter_mut().zip(P.iter()) {
                        *x = p.get(pin);
                    }
                }
                Value::TRUE => push_down(&mut s, p.get(1)),
                _ => {}
            }
        }
        p.set(13, s[0], 4);
    })
}

/// 74194: 4-bit bidirectional universal shift register. nCLR = 1, DSR = 2, A..D = 3..6,
/// DSL = 7, S0 = 9, S1 = 10, CLK = 11, QD..QA = 12..15. S1S0: 00 hold, 01 right (QA→QD),
/// 10 left, 11 load.
pub fn chip_74194() -> Ttl {
    let spec = Spec::new("74194", 16, &[12, 13, 14, 15]);
    // index 0 = D (LSB) ... index 3 = A (MSB)
    let mut s = [Value::FALSE; 4];
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let triggered = edge.rising(p.get(11));
        if p.lo(1) {
            s = [Value::FALSE; 4];
        } else if triggered {
            match (p.hi(10), p.hi(9)) {
                (true, true) => {
                    for (x, pin) in s.iter_mut().zip([6u8, 5, 4, 3]) {
                        *x = p.get(pin);
                    }
                }
                (true, false) => push_up(&mut s, p.get(7)),
                (false, true) => push_down(&mut s, p.get(2)),
                _ => {}
            }
        }
        for (pin, i) in [(15u8, 3usize), (14, 2), (13, 1), (12, 0)] {
            p.set(pin, s[i], 1);
        }
    })
}

/// 74299: 8-bit universal shift/storage register with tri-state I/O. S0 = 1, S1 = 19,
/// nOE1 = 2, nOE2 = 3, CLK = 12, nCLR = 9, SR = 11, SL = 18, QA = 8, QH = 17,
/// I/O A..H = 7,13,6,14,5,15,4,16.
pub fn chip_74299() -> Ttl {
    // index 0 = H ... index 7 = A
    const IO: [u8; 8] = [16, 4, 15, 5, 14, 6, 13, 7];
    let spec = Spec::new("74299", 20, &[8, 17]).inouts(&IO);
    let mut s = [Value::FALSE; 8];
    let mut edge = Edge::new();
    Ttl::new(spec, move |p| {
        let triggered = edge.rising(p.get(12));
        if p.lo(9) {
            s = [Value::FALSE; 8];
        } else if triggered {
            match (p.hi(19), p.hi(1)) {
                (true, true) => {
                    for (x, &pin) in s.iter_mut().zip(IO.iter()) {
                        *x = p.get(pin);
                    }
                }
                (true, false) => push_up(&mut s, p.get(18)),
                (false, true) => push_down(&mut s, p.get(11)),
                _ => {}
            }
        }
        p.set(8, s[7], 1);
        p.set(17, s[0], 1);
        let on = p.lo(2) && p.lo(3) && (p.lo(1) || p.lo(19));
        for (i, &pin) in IO.iter().enumerate() {
            p.set(pin, if on { s[i] } else { Value::UNKNOWN }, 1);
        }
    })
}

fn shift_in(sh: &mut [Value; 8], serial: Value) {
    for stage in (1..8).rev() {
        sh[stage] = sh[stage - 1];
    }
    sh[0] = serial;
}

/// 74594 / 74595: 8-bit shift register with separate 8-bit storage register.
/// Pins: QB..QH = 1..7, QA = 15, QH' = 9, /SRCLR = 10, SRCLK = 11, RCLK = 12, SER = 14, and
/// pin 13 = /OE (74595, tri-state outputs) or /RCLR (74594, clears the storage register).
fn shift_storage(name: &'static str, has_oe: bool) -> Ttl {
    const OUT: [u8; 8] = [15, 1, 2, 3, 4, 5, 6, 7]; // QA..QH
    let spec = Spec::new(name, 16, &[1, 2, 3, 4, 5, 6, 7, 9, 15]);
    let mut shift = [Value::FALSE; 8];
    let mut store = [Value::FALSE; 8];
    let (mut shift_clk, mut store_clk) = (Edge::new(), Edge::new());
    Ttl::new(spec, move |p| {
        let shift_edge = shift_clk.rising(p.get(11));
        let store_edge = store_clk.rising(p.get(12));
        let cleared = p.lo(10);
        let before = shift;
        if cleared {
            shift = [Value::FALSE; 8];
        } else if shift_edge {
            shift_in(&mut shift, p.get(14));
        }
        if !has_oe && p.lo(13) {
            store = [Value::FALSE; 8];
        } else if store_edge {
            // a simultaneous shift edge stores the pre-shift contents
            store = if shift_edge && !cleared { before } else { shift };
        }
        let enabled = !has_oe || p.lo(13);
        for (i, &pin) in OUT.iter().enumerate() {
            p.set(pin, if enabled { store[i] } else { Value::UNKNOWN }, 1);
        }
        p.set(9, shift[7], 1);
    })
}
pub fn chip_74594() -> Ttl {
    shift_storage("74594", false)
}
pub fn chip_74595() -> Ttl {
    shift_storage("74595", true)
}

/// 744094: 8-stage shift-and-store register. STR = 1, D = 2, CP = 3, OE = 15 (active high),
/// QP0..QP3 = 4..7, QP4..QP7 = 14,13,12,11, QS1 = 9 (follows last stage on CP rise),
/// QS2 = 10 (last stage captured on CP fall). Storage follows the shift register while STR is high.
pub fn chip_744094() -> Ttl {
    const QP: [u8; 8] = [4, 5, 6, 7, 14, 13, 12, 11];
    let spec = Spec::new("744094", 16, &[4, 5, 6, 7, 9, 10, 11, 12, 13, 14]);
    let mut shift = [Value::FALSE; 8];
    let mut store = [Value::FALSE; 8];
    let mut qs2 = Value::FALSE;
    let mut rise = Edge::new();
    let mut fall = Edge::new();
    Ttl::new(spec, move |p| {
        let cp = p.get(3);
        if rise.rising(cp) {
            shift_in(&mut shift, p.get(2));
        }
        if fall.falling(cp) {
            qs2 = shift[7];
        }
        if p.hi(1) {
            store = shift;
        }
        let enabled = p.hi(15);
        for (i, &pin) in QP.iter().enumerate() {
            p.set(pin, if enabled { store[i] } else { Value::UNKNOWN }, 1);
        }
        p.set(9, shift[7], 1);
        p.set(10, qs2, 1);
    })
}

pub fn parts() -> Vec<PartDesc> {
    chips! {
        "74173" => chip_74173, "74175" => chip_74175, "74273" => chip_74273,
        "74374" => chip_74374, "74377" => chip_74377, "74164" => chip_74164,
        "74165" => chip_74165, "74166" => chip_74166, "74194" => chip_74194,
        "74299" => chip_74299, "74594" => chip_74594, "74595" => chip_74595,
        "744094" => chip_744094,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::Rig;
    use super::*;

    #[test]
    fn octal_registers() {
        for make in [chip_74273 as fn() -> Ttl, chip_74377, chip_74374] {
            let mut r = Rig::new(make());
            let name = r.name();
            r.put(1, name == "74273"); // 74273: nCLR high; 74377: nCLKen low; 74374: nOE low
            if name == "74377" || name == "74374" {
                r.put(1, false);
            }
            r.put_n(&OCT_D, 0b1010_0110);
            assert_eq!(r.read_n(&OCT_Q), Some(0), "{name} before clock");
            r.pulse(11);
            assert_eq!(r.read_n(&OCT_Q), Some(0b1010_0110), "{name}");
            r.put_n(&OCT_D, 0xFF);
            assert_eq!(r.read_n(&OCT_Q), Some(0b1010_0110), "{name} holds");
        }
        let mut r = Rig::new(chip_74273());
        r.put(1, true);
        r.put_n(&OCT_D, 0xFF);
        r.pulse(11);
        r.put(1, false);
        assert_eq!(r.read_n(&OCT_Q), Some(0));
        let mut r = Rig::new(chip_74377());
        r.put(1, true); // clock disabled
        r.put_n(&OCT_D, 0xFF);
        r.pulse(11);
        assert_eq!(r.read_n(&OCT_Q), Some(0));
        let mut r = Rig::new(chip_74374());
        r.put_n(&OCT_D, 0x55);
        r.pulse(11);
        r.put(1, true);
        assert_eq!(r.get(2), Value::UNKNOWN);
    }

    #[test]
    fn quad_registers() {
        let mut r = Rig::new(chip_74175());
        r.put(1, true);
        r.put_n(&[4, 5, 12, 13], 0b1001);
        r.pulse(9);
        assert_eq!(r.read_n(&[2, 7, 10, 15]), Some(0b1001));
        assert_eq!(r.read_n(&[3, 6, 11, 14]), Some(0b0110));
        r.put(1, false);
        assert_eq!(r.read_n(&[2, 7, 10, 15]), Some(0));

        let mut r = Rig::new(chip_74173());
        r.put_n(&[14, 13, 12, 11], 0b0110);
        r.put(1, false);
        r.put(2, false);
        r.put(9, false);
        r.put(10, false);
        r.pulse(7);
        assert_eq!(r.read_n(&[3, 4, 5, 6]), Some(0b0110));
        r.put(10, true); // load disabled
        r.put_n(&[14, 13, 12, 11], 0b1111);
        r.pulse(7);
        assert_eq!(r.read_n(&[3, 4, 5, 6]), Some(0b0110));
        r.put(1, true); // outputs off
        assert_eq!(r.get(3), Value::UNKNOWN);
        r.put(1, false);
        r.put(15, true); // clear
        assert_eq!(r.read_n(&[3, 4, 5, 6]), Some(0));
    }

    #[test]
    fn shift_registers() {
        let q = [3u8, 4, 5, 6, 10, 11, 12, 13];
        let mut r = Rig::new(chip_74164());
        r.put(9, true);
        r.put(1, true);
        r.put(2, true);
        r.pulse(8);
        r.put(2, false);
        r.pulse(8);
        r.pulse(8);
        assert_eq!(r.read_n(&q), Some(0b100)); // 1 entered, shifted twice
        r.put(9, false);
        assert_eq!(r.read_n(&q), Some(0));

        // 74165: load parallel data, shift out QH first = P7
        let mut r = Rig::new(chip_74165());
        r.put(15, false);
        r.put(1, true);
        r.put_n(&[11, 12, 13, 14, 3, 4, 5, 6], 0b1000_0001);
        r.put(1, false); // load
        r.put(1, true);
        let mut out = vec![];
        for _ in 0..8 {
            out.push(r.hi(9));
            r.pulse(2);
        }
        assert_eq!(out, [true, false, false, false, false, false, false, true]);
        let mut r = Rig::new(chip_74166());
        r.put(9, true);
        r.put(15, false);
        r.put_n(&[2, 3, 4, 5, 10, 11, 12, 14], 0b0000_0010);
        r.pulse(7); // load
        r.put(15, true);
        let mut out = vec![];
        for _ in 0..8 {
            out.push(r.hi(13));
            r.pulse(7);
        }
        assert_eq!(out, [false, false, false, false, false, false, true, false]);

        // 74194 modes
        let mut r = Rig::new(chip_74194());
        r.put(1, true);
        r.put_n(&[3, 4, 5, 6], 0b0001); // A=1
        r.put(9, true);
        r.put(10, true);
        r.pulse(11); // load
        assert_eq!(r.read_n(&[15, 14, 13, 12]), Some(0b0001));
        r.put(10, false); // shift right: QA -> QB
        r.put(2, false);
        r.pulse(11);
        assert_eq!(r.read_n(&[15, 14, 13, 12]), Some(0b0010));
        r.put(9, false);
        r.put(10, true); // shift left, DSL = 1
        r.put(7, true);
        r.pulse(11);
        assert_eq!(r.read_n(&[15, 14, 13, 12]), Some(0b1001));
        r.put(10, false); // hold
        r.pulse(11);
        assert_eq!(r.read_n(&[15, 14, 13, 12]), Some(0b1001));
    }

    #[test]
    fn universal_74299() {
        let io = [7u8, 13, 6, 14, 5, 15, 4, 16];
        let mut r = Rig::new(chip_74299());
        for &p in &io {
            r.set(p, Value::UNKNOWN);
        }
        r.put(9, true);
        r.put(2, false);
        r.put(3, false);
        r.put(1, true);
        r.put(19, true);
        for (i, &p) in io.iter().enumerate() {
            r.put(p, i == 0); // A = 1
        }
        r.pulse(12); // load
        // outputs disabled in load mode (S0=S1=1) -> I/O float... pins driven by test; QA shows A
        assert!(r.hi(8));
        r.put(19, false); // shift right (S0=1)
        for &p in &io {
            r.set(p, Value::UNKNOWN);
        }
        r.put(11, false);
        r.pulse(12);
        assert!(!r.hi(8));
        assert!(r.hi(13)); // B output now 1 (enabled)
    }

    #[test]
    fn shift_and_store() {
        const OUT: [u8; 8] = [15, 1, 2, 3, 4, 5, 6, 7];
        let mut r = Rig::new(chip_74595());
        r.put(10, true);
        r.put(13, false);
        // shift in 1,0,1,1,0,0,1,0 (first bit ends in QH)
        let bits = [true, false, true, true, false, false, true, false];
        for &b in &bits {
            r.put(14, b);
            r.pulse(11);
        }
        assert_eq!(r.read_n(&OUT), Some(0)); // not stored yet
        r.pulse(12);
        // QA = last bit shifted in (index 0), QH = first
        let expect: u64 = bits.iter().rev().enumerate().map(|(i, &b)| (b as u64) << i).sum();
        assert_eq!(r.read_n(&OUT), Some(expect));
        r.put(13, true);
        assert_eq!(r.get(15), Value::UNKNOWN);
        r.put(13, false);
        r.put(10, false);
        r.pulse(12);
        assert_eq!(r.read_n(&OUT), Some(0));

        let mut r = Rig::new(chip_74594());
        r.put(10, true);
        r.put(13, true);
        r.put(14, true);
        r.pulse(11);
        r.pulse(12);
        assert!(r.hi(15));
        r.put(13, false); // storage clear
        assert!(!r.hi(15));

        let mut r = Rig::new(chip_744094());
        r.put(15, true);
        r.put(2, true);
        r.pulse(3);
        assert!(!r.hi(4)); // not strobed
        r.put(1, true);
        assert!(r.hi(4));
        r.put(15, false);
        assert_eq!(r.get(4), Value::UNKNOWN);
    }
}
