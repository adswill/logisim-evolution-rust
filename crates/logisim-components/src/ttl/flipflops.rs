//! Flip-flops: 7474 (dual D), 7476 (dual JK, falling edge), 74107 (dual JK, falling edge,
//! clear only).

use super::*;
use logisim_core::Value;

fn complement(v: Value) -> Value {
    match v {
        Value::TRUE => Value::FALSE,
        Value::FALSE => Value::TRUE,
        Value::ERROR => Value::ERROR,
        _ => Value::UNKNOWN,
    }
}

/// Next state of a JK flip-flop on its active edge, resolving X inputs conservatively: if every
/// 0/1 reading of an unknown J or K gives the same result use it, otherwise X (E if an input is E).
pub fn jk_next(q: Value, j: Value, k: Value) -> Value {
    fn levels(v: Value) -> &'static [bool] {
        match v {
            Value::TRUE => &[true],
            Value::FALSE => &[false],
            _ => &[false, true],
        }
    }
    fn defined(q: Value, j: bool, k: bool) -> Value {
        match (j, k) {
            (true, true) => complement(q),
            (true, false) => Value::TRUE,
            (false, true) => Value::FALSE,
            _ => q,
        }
    }
    let agreed = defined(q, levels(j)[0], levels(k)[0]);
    for &jh in levels(j) {
        for &kh in levels(k) {
            if defined(q, jh, kh) != agreed {
                return if j == Value::ERROR || k == Value::ERROR {
                    Value::ERROR
                } else {
                    Value::UNKNOWN
                };
            }
        }
    }
    agreed
}

/// 7474: dual D flip-flop with active-low preset/clear, rising edge. Both low → X.
pub fn chip_7474() -> Ttl {
    let spec = Spec::new("7474", 14, &[5, 6, 8, 9]);
    let mut q = [Value::FALSE; 2];
    let mut edge = [Edge::new(); 2];
    Ttl::new(spec, move |p| {
        // (nCLR, D, CLK, nPRE, Q, nQ)
        for (k, &(clr, d, clk, pre, qo, nqo)) in [
            (1u8, 2u8, 3u8, 4u8, 5u8, 6u8),
            (13, 12, 11, 10, 9, 8),
        ]
        .iter()
        .enumerate()
        {
            let trig = edge[k].rising(p.get(clk));
            q[k] = match (p.lo(clr), p.lo(pre)) {
                (true, true) => Value::UNKNOWN,
                (true, false) => Value::FALSE,
                (false, true) => Value::TRUE,
                _ if trig => p.get(d),
                _ => q[k],
            };
            p.set(qo, q[k], 8);
            p.set(nqo, !q[k], 8);
        }
    })
}

/// 7476: dual JK flip-flop, falling edge, active-low preset and clear (both low → X).
pub fn chip_7476() -> Ttl {
    let spec = Spec::new("7476", 16, &[10, 11, 14, 15]).power(5, 13);
    let mut q = [Value::FALSE; 2];
    let mut edge = [Edge::new(); 2];
    Ttl::new(spec, move |p| {
        // (CLK, nPRE, nCLR, J, K, Q, nQ)
        for (n, &(clk, pre, clr, j, k, qo, nqo)) in [
            (1u8, 2u8, 3u8, 4u8, 16u8, 15u8, 14u8),
            (6, 7, 8, 9, 12, 11, 10),
        ]
        .iter()
        .enumerate()
        {
            let trig = edge[n].falling(p.get(clk));
            q[n] = match (p.lo(pre), p.lo(clr)) {
                (true, true) => Value::UNKNOWN,
                (true, false) => Value::TRUE,
                (false, true) => Value::FALSE,
                _ if trig => {
                    let (jv, kv) = (p.get(j), p.get(k));
                    // Logisim: only defined J/K combinations change the state.
                    match (jv, kv) {
                        (Value::TRUE, Value::TRUE) => !q[n],
                        (Value::TRUE, Value::FALSE) => Value::TRUE,
                        (Value::FALSE, Value::TRUE) => Value::FALSE,
                        _ => q[n],
                    }
                }
                _ => q[n],
            };
            p.set(qo, q[n], 8);
            p.set(nqo, !q[n], 8);
        }
    })
}

/// 74107: dual JK flip-flop, falling edge, active-low clear.
pub fn chip_74107() -> Ttl {
    let spec = Spec::new("74107", 14, &[2, 3, 5, 6]);
    let mut q = [Value::FALSE; 2];
    let mut edge = [Edge::new(); 2];
    Ttl::new(spec, move |p| {
        // (J, K, CLK, nCLR, Q, nQ)
        for (n, &(j, k, clk, clr, qo, nqo)) in [
            (1u8, 4u8, 12u8, 13u8, 3u8, 2u8),
            (8, 11, 9, 10, 5, 6),
        ]
        .iter()
        .enumerate()
        {
            let trig = edge[n].falling(p.get(clk));
            q[n] = match p.get(clr) {
                Value::FALSE => Value::FALSE,
                Value::ERROR => Value::ERROR,
                Value::TRUE if trig => jk_next(q[n], p.get(j), p.get(k)),
                Value::TRUE => q[n],
                _ => Value::UNKNOWN,
            };
            p.set(qo, q[n], 8);
            p.set(nqo, complement(q[n]), 8);
        }
    })
}

pub fn parts() -> Vec<PartDesc> {
    chips! { "7474" => chip_7474, "7476" => chip_7476, "74107" => chip_74107 }
}

#[cfg(test)]
mod tests {
    use super::super::testing::Rig;
    use super::*;

    #[test]
    fn d_flip_flop() {
        let mut r = Rig::new(chip_7474());
        r.put(4, true); // nPRE
        r.put(1, true); // nCLR
        r.put(2, true); // D
        assert!(!r.hi(5));
        r.pulse(3);
        assert!(r.hi(5) && !r.hi(6));
        r.put(2, false);
        assert!(r.hi(5)); // no edge yet
        r.pulse(3);
        assert!(!r.hi(5) && r.hi(6));
        r.put(4, false); // preset async
        assert!(r.hi(5));
        r.put(1, false); // both low → X
        assert_eq!(r.get(5), Value::UNKNOWN);
        r.put(4, true);
        assert!(!r.hi(5)); // clear wins now
    }

    #[test]
    fn d_flip_flop_second_half() {
        let mut r = Rig::new(chip_7474());
        r.put(13, true);
        r.put(10, true);
        r.put(12, true);
        r.pulse(11);
        assert!(r.hi(9) && !r.hi(8));
    }

    #[test]
    fn jk_7476() {
        let mut r = Rig::new(chip_7476());
        for p in [2, 3] {
            r.put(p, true);
        }
        r.put(4, true); // J
        r.put(16, false); // K
        r.put(1, true);
        r.put(1, false); // falling edge sets
        assert!(r.hi(15) && !r.hi(14));
        r.put(16, true); // toggle
        r.pulse(1);
        r.put(1, false);
        assert!(!r.hi(15));
        r.put(2, false); // preset
        assert!(r.hi(15));
        r.put(3, false);
        assert_eq!(r.get(15), Value::UNKNOWN);
    }

    #[test]
    fn jk_74107_toggles_and_clears() {
        let mut r = Rig::new(chip_74107());
        r.put(13, true);
        r.put(1, true);
        r.put(4, true);
        r.put(12, true);
        r.put(12, false);
        assert!(r.hi(3) && !r.hi(2));
        r.pulse(12);
        assert!(!r.hi(3));
        r.pulse(12);
        assert!(r.hi(3));
        r.put(13, false);
        assert!(!r.hi(3));
    }

    #[test]
    fn jk_next_unknowns() {
        assert_eq!(jk_next(Value::FALSE, Value::UNKNOWN, Value::FALSE), Value::UNKNOWN);
        assert_eq!(jk_next(Value::TRUE, Value::FALSE, Value::FALSE), Value::TRUE);
        assert_eq!(jk_next(Value::FALSE, Value::TRUE, Value::UNKNOWN), Value::TRUE);
        assert_eq!(jk_next(Value::TRUE, Value::TRUE, Value::UNKNOWN), Value::UNKNOWN);
    }
}
