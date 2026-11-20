//! Arithmetic, comparators, display decoder and other chips: 7447, 7485, 74257, 74266, 74283,
//! 74688, 744075, 747266 (+ the larger parts in the second half of this file).

use super::*;

/// 7447 segment table, active-low outputs a..g for digits 0..15 (`1` = segment off).
const SEG_7447: [[u8; 7]; 16] = [
    [0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 1, 1, 1, 1],
    [0, 0, 1, 0, 0, 1, 0],
    [0, 0, 0, 0, 1, 1, 0],
    [1, 0, 0, 1, 1, 0, 0],
    [0, 1, 0, 0, 1, 0, 0],
    [1, 1, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 0, 0],
    [1, 1, 1, 0, 0, 1, 0],
    [1, 1, 0, 0, 1, 1, 0],
    [1, 0, 1, 1, 1, 0, 0],
    [0, 1, 1, 0, 1, 0, 0],
    [1, 1, 1, 0, 0, 0, 0],
    [1, 1, 1, 1, 1, 1, 1],
];

/// 7447: BCD to 7-segment decoder/driver, active-low outputs. A..D = 7,1,2,6, /LT = 3,
/// /RBI = 5, /BI//RBO = 4 (open collector, also an input), outputs a..g = 13,12,11,10,9,15,14.
pub fn chip_7447() -> Ttl {
    const SEG: [u8; 7] = [13, 12, 11, 10, 9, 15, 14];
    let spec = Spec::new("7447", 16, &[9, 10, 11, 12, 13, 14, 15]).inouts(&[4]);
    Ttl::new(spec, |p| {
        let value = p.read(&[7, 1, 2, 6]);
        let blank_zero = p.lo(5) && value == Some(0);
        match value {
            None => p.write_unknown(&SEG, 1),
            Some(mut v) => {
                if p.lo(4) {
                    v = 15;
                } else if p.lo(3) {
                    v = 8;
                } else if blank_zero {
                    v = 15;
                }
                for (i, &pin) in SEG.iter().enumerate() {
                    p.set_bool(pin, SEG_7447[v as usize][i] == 1, 1);
                }
            }
        }
        let rbo = if blank_zero && p.get(3) != Value::FALSE {
            Value::FALSE
        } else {
            Value::UNKNOWN
        };
        p.set(4, rbo, 1);
    })
}

/// 7485: 4-bit magnitude comparator. A0..A3 = 10,12,13,15; B0..B3 = 9,11,14,1;
/// cascade in: A<B = 2, A=B = 3, A>B = 4; out: A>B = 5, A=B = 6, A<B = 7.
pub fn chip_7485() -> Ttl {
    let spec = Spec::new("7485", 16, &[5, 6, 7]);
    Ttl::new(spec, |p| {
        let a = p.read(&[10, 12, 13, 15]).unwrap_or(0);
        let b = p.read(&[9, 11, 14, 1]).unwrap_or(0);
        let (lt_in, eq_in, gt_in) = (p.hi(2), p.hi(3), p.hi(4));
        let (gt, eq, lt) = if a > b {
            (true, false, false)
        } else if a < b {
            (false, false, true)
        } else if eq_in {
            (false, true, false)
        } else if lt_in && gt_in {
            (false, false, false)
        } else if lt_in {
            (false, false, true)
        } else if gt_in {
            (true, false, false)
        } else {
            (true, false, true)
        };
        p.set_bool(5, gt, 1);
        p.set_bool(6, eq, 1);
        p.set_bool(7, lt, 1);
    })
}

/// 74257: quad 2-to-1 multiplexer with tri-state outputs. S = 1, /OE = 15, (A, B, Y) =
/// (2,3,4), (5,6,7), (11,10,9), (14,13,12).
pub fn chip_74257() -> Ttl {
    let spec = Spec::new("74257", 16, &[4, 7, 9, 12]);
    Ttl::new(spec, |p| {
        let (oe, sel) = (p.get(15), p.get(1));
        for (a, b, y) in [(2u8, 3u8, 4u8), (5, 6, 7), (11, 10, 9), (14, 13, 12)] {
            let (a, b) = (p.get(a), p.get(b));
            let driven = match sel {
                Value::TRUE => b,
                Value::FALSE => a,
                _ if a == b => a,
                _ if [sel, a, b].contains(&Value::ERROR) => Value::ERROR,
                _ => Value::UNKNOWN,
            };
            let out = match oe {
                Value::TRUE => Value::UNKNOWN,
                Value::FALSE => driven,
                _ if driven == Value::UNKNOWN && oe == Value::UNKNOWN => Value::UNKNOWN,
                _ if oe == Value::ERROR || driven == Value::ERROR => Value::ERROR,
                _ => Value::UNKNOWN,
            };
            p.set(y, out, 1);
        }
    })
}

/// 74266: quad XNOR with open-collector outputs (pins as 7486's neighbours: 1/2→3, 5/6→4,
/// 8/9→10, 12/13→11).
pub fn chip_74266() -> Ttl {
    let spec = Spec::new("74266", 14, &[3, 4, 10, 11]);
    Ttl::new(spec, |p| {
        for (a, b, y) in [(1u8, 2u8, 3u8), (5, 6, 4), (8, 9, 10), (12, 13, 11)] {
            let xnor = !p.get(a).xor(p.get(b));
            p.set(y, if xnor == Value::TRUE { Value::UNKNOWN } else { Value::FALSE }, 1);
        }
    })
}

/// 747266: quad XNOR (push-pull), 7400 pinout.
pub fn chip_747266() -> Ttl {
    let spec = Spec::new("747266", 14, &[3, 6, 8, 11]);
    Ttl::new(spec, |p| {
        for (y, a, b) in [(3u8, 1u8, 2u8), (6, 4, 5), (8, 10, 9), (11, 13, 12)] {
            let v = !p.get(a).xor(p.get(b));
            p.set(y, v, 1);
        }
    })
}

/// 74283: 4-bit binary full adder. A1..A4 = 5,3,14,12; B1..B4 = 6,2,15,11; C0 = 7;
/// Σ1..Σ4 = 4,1,13,10; C4 = 9.
pub fn chip_74283() -> Ttl {
    let spec = Spec::new("74283", 16, &[1, 4, 9, 10, 13]);
    Ttl::new(spec, |p| {
        let a = p.read(&[5, 3, 14, 12]).unwrap_or(0);
        let b = p.read(&[6, 2, 15, 11]).unwrap_or(0);
        let c = p.hi(7) as u64;
        let sum = a + b + c;
        p.write(&[4, 1, 13, 10, 9], sum, 1);
    })
}

/// 74688: 8-bit equality comparator. P0..P7 = 2,4,6,8,11,13,15,17; Q0..Q7 = 3,5,7,9,12,14,16,18;
/// /E = 1; output /(P=Q) = 19 (low when enabled and equal).
pub fn chip_74688() -> Ttl {
    let spec = Spec::new("74688", 20, &[19]);
    Ttl::new(spec, |p| {
        let pq = [(2u8, 3u8), (4, 5), (6, 7), (8, 9), (11, 12), (13, 14), (15, 16), (17, 18)];
        let equal = pq.iter().all(|&(a, b)| {
            let (x, y) = (p.get(a), p.get(b));
            (x == Value::TRUE || x == Value::FALSE) && x == y
        });
        p.set_bool(19, !(p.lo(1) && equal), 1);
    })
}

/// 744075: triple 3-input OR. 1A,1B,1C = 3,4,5 → 6; 2A,2B,2C = 1,2,8 → 9; 3A,3B,3C = 11,12,13 → 10.
pub fn chip_744075() -> Ttl {
    let spec = Spec::new("744075", 14, &[6, 9, 10]);
    Ttl::new(spec, |p| {
        for (a, b, c, y) in [(3u8, 4u8, 5u8, 6u8), (1, 2, 8, 9), (11, 12, 13, 10)] {
            p.set(y, p.get(a).or(p.get(b)).or(p.get(c)), 1);
        }
    })
}

pub fn parts() -> Vec<PartDesc> {
    chips! {
        "7447" => chip_7447, "7485" => chip_7485, "74257" => chip_74257,
        "74266" => chip_74266, "74283" => chip_74283, "74688" => chip_74688,
        "744075" => chip_744075, "747266" => chip_747266,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::Rig;
    use super::*;

    #[test]
    fn seven_segment_7447() {
        let mut r = Rig::new(chip_7447());
        r.set(4, Value::UNKNOWN);
        r.put(3, true);
        r.put(5, true);
        for v in 0..16u64 {
            r.put_n(&[7, 1, 2, 6], v);
            let got: Vec<u8> = [13u8, 12, 11, 10, 9, 15, 14].iter().map(|&p| r.hi(p) as u8).collect();
            assert_eq!(got, SEG_7447[v as usize], "digit {v}");
        }
        // lamp test: all segments on
        r.put_n(&[7, 1, 2, 6], 3);
        r.put(3, false);
        assert!([13u8, 12, 11, 10, 9, 15, 14].iter().all(|&p| !r.hi(p)));
        r.put(3, true);
        // ripple blanking: RBI low and zero → blank and RBO low
        r.put_n(&[7, 1, 2, 6], 0);
        r.put(5, false);
        assert!([13u8, 12, 11, 10, 9, 15, 14].iter().all(|&p| r.hi(p)));
        assert_eq!(r.get(4), Value::FALSE);
    }

    #[test]
    fn comparator_7485() {
        let mut r = Rig::new(chip_7485());
        let a = [10u8, 12, 13, 15];
        let b = [9u8, 11, 14, 1];
        for x in 0..16u64 {
            for y in [0u64, 5, 15] {
                r.put_n(&a, x);
                r.put_n(&b, y);
                r.put(3, true); // A=B cascade in
                assert_eq!((r.hi(5), r.hi(6), r.hi(7)), (x > y, x == y, x < y), "{x} {y}");
            }
        }
        r.put_n(&a, 3);
        r.put_n(&b, 3);
        r.put(3, false);
        assert!(r.hi(5) && !r.hi(6) && r.hi(7)); // no cascade input: both high
    }

    #[test]
    fn adder_74283() {
        let mut r = Rig::new(chip_74283());
        for (x, y, c) in [(0u64, 0u64, 0u64), (7, 8, 1), (15, 15, 1), (9, 6, 0), (15, 1, 0)] {
            r.put_n(&[5, 3, 14, 12], x);
            r.put_n(&[6, 2, 15, 11], y);
            r.put(7, c == 1);
            assert_eq!(r.read_n(&[4, 1, 13, 10, 9]), Some(x + y + c), "{x}+{y}+{c}");
        }
    }

    #[test]
    fn xnor_equality_mux_or() {
        let mut r = Rig::new(chip_747266());
        r.put(1, true);
        r.put(2, true);
        assert!(r.hi(3));
        let mut r = Rig::new(chip_74266());
        r.put(1, true);
        assert_eq!(r.get(3), Value::FALSE);
        r.put(2, true);
        assert_eq!(r.get(3), Value::UNKNOWN);

        let mut r = Rig::new(chip_74688());
        r.put(1, false);
        assert!(!r.hi(19)); // all zeros equal
        r.put(2, true);
        assert!(r.hi(19));
        r.put(3, true);
        assert!(!r.hi(19));
        r.put(1, true);
        assert!(r.hi(19));

        let mut r = Rig::new(chip_744075());
        r.put(8, true);
        assert!(r.hi(9) && !r.hi(6) && !r.hi(10));

        let mut r = Rig::new(chip_74257());
        r.put(15, false);
        r.put(2, true);
        assert!(r.hi(4));
        r.put(1, true);
        assert!(!r.hi(4));
        r.put(15, true);
        assert_eq!(r.get(4), Value::UNKNOWN);
    }
}
