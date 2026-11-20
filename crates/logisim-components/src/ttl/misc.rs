//! Arithmetic, comparators, display decoder and other chips: 7447, 7485, 74257, 74266, 74283,
//! 74688, 744075, 747266 (+ the larger parts in the second half of this file).

use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

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

/// 7487: 4-bit true/complement, zero/one element. C = 1, B = 8, A1..A4 = 2,5,10,13,
/// Y1..Y4 = 3,6,9,12. B high: Y = !C; B low: C high → Y = A, C low → Y = !A.
pub fn chip_7487() -> Ttl {
    let spec = Spec::new("7487", 14, &[3, 6, 9, 12]).unused(&[4, 11]);
    Ttl::new(spec, |p| {
        let (b, c) = (p.get(8), p.get(1));
        for (a, y) in [(2u8, 3u8), (5, 6), (10, 9), (13, 12)] {
            let v = match (b, c) {
                (Value::TRUE, _) => !c,
                (Value::FALSE, Value::TRUE) => p.get(a),
                (Value::FALSE, Value::FALSE) => !p.get(a),
                _ => Value::ERROR,
            };
            p.set(y, v, 1);
        }
    })
}

/// 74182: look-ahead carry generator. /G0../G3 = 3,1,14,5; /P0../P3 = 4,2,15,6; Cn = 13;
/// outputs /P = 7, /G = 10, Cn+x = 12, Cn+y = 11, Cn+z = 9.
pub fn chip_74182() -> Ttl {
    let spec = Spec::new("74182", 16, &[7, 10, 12, 11, 9]);
    Ttl::new(spec, |p| {
        let pp = [!p.hi(4), !p.hi(2), !p.hi(15), !p.hi(6)];
        let g = [!p.hi(3), !p.hi(1), !p.hi(14), !p.hi(5)];
        let ci = p.hi(13);
        let po = pp[3] && pp[2] && pp[1] && pp[0];
        let go = g[3]
            || (pp[3] && g[2])
            || (pp[3] && pp[2] && g[1])
            || (pp[3] && pp[2] && pp[1] && g[0]);
        let cx = g[0] || (pp[0] && ci);
        let cy = g[1] || (pp[1] && g[0]) || (pp[1] && pp[0] && ci);
        let cz = g[2] || (pp[2] && g[1]) || (pp[2] && pp[1] && g[0]) || (pp[2] && pp[1] && pp[0] && ci);
        p.set_bool(7, !po, 1);
        p.set_bool(10, !go, 1);
        p.set_bool(12, cx, 1);
        p.set_bool(11, cy, 1);
        p.set_bool(9, cz, 1);
    })
}

/// PAL-style AND-OR plane from Logisim's ALU ports (active-low result): `products` are bit masks
/// over `inputs`.
fn pal(inputs: &[bool], products: &[u32]) -> bool {
    let or = products.iter().any(|&prod| {
        (0..32).all(|j| prod & (1 << j) == 0 || inputs.get(j).copied().unwrap_or(false))
    });
    !or
}

/// 74181: 4-bit ALU / function generator (active-high data). A0..A3 = 2,23,21,19;
/// B0..B3 = 1,22,20,18; S0..S3 = 6,5,4,3; Cn = 7; M = 8; F0..F3 = 9,10,11,13; A=B = 14
/// (open collector); Cn+4 = 16; P = 15; G = 17.
pub fn chip_74181() -> Ttl {
    let spec = Spec::new("74181", 24, &[9, 10, 11, 13, 14, 15, 16, 17]);
    Ttl::new(spec, |p| {
        let a: Vec<bool> = [2u8, 23, 21, 19].iter().map(|&x| p.hi(x)).collect();
        let b: Vec<bool> = [1u8, 22, 20, 18].iter().map(|&x| p.hi(x)).collect();
        let s: Vec<bool> = [6u8, 5, 4, 3].iter().map(|&x| p.hi(x)).collect();
        let (ci, m) = (p.hi(7), p.hi(8));
        let mut l1 = vec![];
        l1.extend(&a);
        l1.extend(&b);
        l1.extend(b.iter().map(|x| !x));
        l1.extend(&s);
        let x: Vec<bool> = [
            [0x0000_8011u32, 0x0000_4101],
            [0x0000_8022, 0x0000_4202],
            [0x0000_8044, 0x0000_4404],
            [0x0000_8088, 0x0000_4808],
        ]
        .iter()
        .map(|t| pal(&l1, t))
        .collect();
        let y: Vec<bool> = [
            [0x0000_2100u32, 0x0000_1010, 0x0000_0001],
            [0x0000_2200, 0x0000_1020, 0x0000_0002],
            [0x0000_2400, 0x0000_1040, 0x0000_0004],
            [0x0000_2800, 0x0000_1080, 0x0000_0008],
        ]
        .iter()
        .map(|t| pal(&l1, t))
        .collect();
        let mut l2 = vec![];
        l2.extend(&x);
        l2.extend(&y);
        l2.push(!m);
        l2.push(ci);
        let z = [
            pal(&l2, &[0x300]),
            pal(&l2, &[0x301, 0x110]),
            pal(&l2, &[0x303, 0x112, 0x120]),
            pal(&l2, &[0x307, 0x116, 0x124, 0x140]),
        ];
        let pp = pal(&l2, &[0x0f]);
        let g = pal(&l2, &[0x80, 0x48, 0x2c, 0x1e]);
        let co = !pal(&l2, &[0x20f]) || !g;
        let f: Vec<bool> = (0..4).map(|i| x[i] ^ y[i] ^ z[i]).collect();
        let eq = f.iter().all(|&v| v);
        p.set_bool(15, pp, 1);
        p.set_bool(17, g, 1);
        p.set_bool(16, co, 1);
        p.set_open(14, eq, 1);
        for (pin, v) in [(13u8, f[3]), (11, f[2]), (10, f[1]), (9, f[0])] {
            p.set_bool(pin, v, 1);
        }
    })
}

/// 74381: 4-bit ALU/function generator. A0..A3 = 3,1,19,17; B0..B3 = 4,2,18,16;
/// S0..S2 = 5,6,7; Cn = 15; F0..F3 = 8,9,11,12; /P = 14; /G = 13.
/// S: 000 clear, 001 B−A, 010 A−B, 011 A+B, 100 XOR, 101 OR, 110 AND, 111 preset.
pub fn chip_74381() -> Ttl {
    let spec = Spec::new("74381", 20, &[8, 9, 11, 12, 13, 14]);
    Ttl::new(spec, |p| {
        let a: Vec<bool> = [3u8, 1, 19, 17].iter().map(|&x| p.hi(x)).collect();
        let b: Vec<bool> = [4u8, 2, 18, 16].iter().map(|&x| p.hi(x)).collect();
        let (s0, s1, s2) = (p.hi(5), p.hi(6), p.hi(7));
        let ci = p.hi(15);
        let u = s0 || s1;
        let v = s1 || s2;
        let w = s0 || !s1;
        let x = !(s0 && s1) || s2;
        let y = (s0 && s1) || !s2;
        let z = !s2 && u;
        let mut l1 = vec![];
        l1.extend(&a);
        l1.extend(a.iter().map(|q| !q));
        l1.extend(&b);
        l1.extend(b.iter().map(|q| !q));
        l1.extend([y, x, w, v, u]);
        let j: Vec<bool> = [
            [0x000c_1010u32, 0x0016_1001, 0x000a_0110],
            [0x000c_2020, 0x0016_2002, 0x000a_0220],
            [0x000c_4040, 0x0016_4004, 0x000a_0440],
            [0x000c_8080, 0x0016_8008, 0x000a_0880],
        ]
        .iter()
        .map(|t| pal(&l1, t))
        .collect();
        let k: Vec<bool> = [
            [0x0013_1010u32, 0x000c_1001, 0x000c_0110, 0x0012_0101],
            [0x0013_2020, 0x000c_2002, 0x000c_0220, 0x0012_0202],
            [0x0013_4040, 0x000c_4004, 0x000c_0440, 0x0012_0404],
            [0x0013_8080, 0x000c_8008, 0x000c_0880, 0x0012_0808],
        ]
        .iter()
        .map(|t| pal(&l1, t))
        .collect();
        let mut l2 = vec![];
        l2.extend(&j);
        l2.extend(&k);
        l2.push(z);
        l2.push(ci);
        let l = [
            pal(&l2, &[0x300]),
            pal(&l2, &[0x301, 0x111]),
            pal(&l2, &[0x303, 0x113, 0x122]),
            pal(&l2, &[0x307, 0x117, 0x126, 0x144]),
        ];
        let pp = pal(&l2, &[0x0f]);
        let g = pal(&l2, &[0x88, 0x4c, 0x2e, 0x1f]);
        p.set_bool(14, pp, 1);
        p.set_bool(13, g, 1);
        for (pin, i) in [(12u8, 3usize), (11, 2), (9, 1), (8, 0)] {
            p.set_bool(pin, l[i] ^ k[i], 1);
        }
    })
}

fn combine_levels(selected: Value, idle: Value) -> Value {
    if selected == idle {
        selected
    } else if selected == Value::ERROR
        || idle == Value::ERROR
        || (selected.is_fully_defined() && idle.is_fully_defined())
    {
        Value::ERROR
    } else {
        Value::UNKNOWN
    }
}

/// 74259: 8-bit addressable latch. A0..A2 = 1,2,3; D = 13; /LE = 14; /MR = 15;
/// Q0..Q3 = 4..7, Q4..Q7 = 9..12.
pub fn chip_74259() -> Ttl {
    const Q: [u8; 8] = [4, 5, 6, 7, 9, 10, 11, 12];
    let spec = Spec::new("74259", 16, &Q);
    let mut st = [Value::FALSE; 8];
    Ttl::new(spec, move |p| {
        let (mr, le) = (p.get(15), p.get(14));
        let write = |p: &Pins, st: &mut [Value; 8], clear_unselected: bool| {
            let d = p.get(13);
            for (i, s) in st.iter_mut().enumerate() {
                let idle = if clear_unselected { Value::FALSE } else { *s };
                let addr = [1u8, 2, 3].map(|a| p.get(a));
                let excluded = addr.iter().enumerate().any(|(place, v)| {
                    let want = (i >> place) & 1 == 1;
                    (want && *v == Value::FALSE) || (!want && *v == Value::TRUE)
                });
                let known = addr.iter().all(|v| *v == Value::TRUE || *v == Value::FALSE);
                *s = if excluded {
                    idle
                } else if known {
                    d
                } else {
                    combine_levels(d, idle)
                };
            }
        };
        if mr == Value::FALSE && le == Value::TRUE {
            st = [Value::FALSE; 8];
        } else if mr == Value::FALSE && le == Value::FALSE {
            write(p, &mut st, true);
        } else if mr == Value::TRUE && le == Value::FALSE {
            write(p, &mut st, false);
        }
        for (i, &q) in Q.iter().enumerate() {
            p.set(q, st[i], 1);
        }
    })
}

/// 74670: 4×4 register file with tri-state outputs. D1..D4 = 15,1,2,3; write address
/// WB,WA = 13,14; read address RB,RA = 4,5; /WE = 12; /RE = 11; Q1..Q4 = 10,9,7,6.
/// Writing is level-sensitive while /WE is low.
pub fn chip_74670() -> Ttl {
    let spec = Spec::new("74670", 16, &[10, 9, 7, 6]);
    let mut words = [[Value::FALSE; 4]; 4];
    Ttl::new(spec, move |p| {
        let addr = |msb: u8, lsb: u8| (p.hi(msb) as usize) << 1 | p.hi(lsb) as usize;
        if p.lo(12) {
            let w = addr(13, 14);
            words[w] = [p.get(15), p.get(1), p.get(2), p.get(3)];
        }
        let r = addr(4, 5);
        let on = p.lo(11);
        for (i, &q) in [10u8, 9, 7, 6].iter().enumerate() {
            p.set(q, if on { words[r][i] } else { Value::UNKNOWN }, 1);
        }
    })
}

/// 74123: dual retriggerable monostable multivibrator.
///
/// Logisim times the pulse from an Rext/Cext RC model in wall-clock nanoseconds; this engine has
/// no analog time, so the pulse length is the integer option `pulse_ticks` (simulation clock
/// ticks, default 2). Pins: 1A = 1, 1B = 2, /1CLR = 3, /1Q = 4, 2Q = 5, 2A = 9, 2B = 10,
/// /2CLR = 11, /2Q = 12, 1Q = 13. Trigger: B rising with A low, A falling with B high, or
/// CLR rising with A low and B high. CLR low ends the pulse.
pub fn chip_74123() -> Ttl {
    #[derive(Clone, Copy)]
    struct Half {
        a: Value,
        b: Value,
        clr: Value,
        remaining: i64,
    }
    let spec = Spec::new("74123", 16, &[4, 5, 12, 13]).unused(&[6, 7, 14, 15]);
    let ticks = Rc::new(Cell::new(2i64));
    let state = Rc::new(RefCell::new([Half { a: Value::FALSE, b: Value::TRUE, clr: Value::TRUE, remaining: 0 }; 2]));
    // (A, B, CLR, Q, /Q)
    const HALVES: [(u8, u8, u8, u8, u8); 2] = [(1, 2, 3, 13, 4), (9, 10, 11, 5, 12)];
    let publish = |p: &mut Pins, h: usize, remaining: i64| {
        let (.., q, nq) = HALVES[h];
        p.set_bool(q, remaining > 0, 4);
        p.set_bool(nq, remaining <= 0, 4);
    };
    let st = state.clone();
    let tk = ticks.clone();
    let propagate = move |p: &mut Pins| {
        let mut s = st.borrow_mut();
        for (h, &(a, b, clr, ..)) in HALVES.iter().enumerate() {
            let (na, nb, nc) = (p.get(a), p.get(b), p.get(clr));
            let old = s[h];
            let rising_b = old.b == Value::FALSE && nb == Value::TRUE && na == Value::FALSE;
            let falling_a = old.a == Value::TRUE && na == Value::FALSE && nb == Value::TRUE;
            let rising_clr = old.clr == Value::FALSE && nc == Value::TRUE && na == Value::FALSE && nb == Value::TRUE;
            s[h] = Half { a: na, b: nb, clr: nc, remaining: old.remaining };
            if nc == Value::FALSE {
                s[h].remaining = 0;
            } else if rising_b || falling_a || rising_clr {
                s[h].remaining = tk.get();
            }
            publish(p, h, s[h].remaining);
        }
    };
    let st = state;
    let tick = move |p: &mut Pins| {
        let mut s = st.borrow_mut();
        for h in 0..2 {
            if s[h].remaining > 0 {
                s[h].remaining -= 1;
                publish(p, h, s[h].remaining);
            }
        }
    };
    Ttl::new(spec, propagate).on_tick(tick).with_int_option("pulse_ticks", ticks, 1, 1_000_000)
}

pub fn parts() -> Vec<PartDesc> {
    chips! {
        "7447" => chip_7447, "7485" => chip_7485, "74257" => chip_74257,
        "74266" => chip_74266, "74283" => chip_74283, "74688" => chip_74688,
        "744075" => chip_744075, "747266" => chip_747266, "7487" => chip_7487, "74123" => chip_74123,
        "74181" => chip_74181, "74182" => chip_74182, "74259" => chip_74259,
        "74381" => chip_74381, "74670" => chip_74670,
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

    fn alu181(r: &mut Rig, a: u64, b: u64, s: u64, m: bool, cn: bool) -> u64 {
        r.put_n(&[2, 23, 21, 19], a);
        r.put_n(&[1, 22, 20, 18], b);
        r.put_n(&[6, 5, 4, 3], s);
        r.put(8, m);
        r.put(7, cn);
        r.read_n(&[9, 10, 11, 13]).unwrap()
    }

    #[test]
    fn alu_74181() {
        let mut r = Rig::new(chip_74181());
        for a in 0..16u64 {
            for b in [0u64, 3, 9, 15] {
                assert_eq!(alu181(&mut r, a, b, 0b0000, true, true), !a & 15, "NOT A");
                assert_eq!(alu181(&mut r, a, b, 0b0110, true, true), a ^ b, "XOR");
                assert_eq!(alu181(&mut r, a, b, 0b1011, true, true), a & b, "AND");
                assert_eq!(alu181(&mut r, a, b, 0b1110, true, true), a | b, "OR");
                assert_eq!(alu181(&mut r, a, b, 0b1001, false, true), (a + b) & 15, "A+B");
                assert_eq!(alu181(&mut r, a, b, 0b1001, false, false), (a + b + 1) & 15, "A+B+1");
                assert_eq!(alu181(&mut r, a, b, 0b0110, false, true), a.wrapping_sub(b).wrapping_sub(1) & 15, "A-B-1");
            }
        }
    }

    #[test]
    fn alu_74381() {
        let mut r = Rig::new(chip_74381());
        let mut run = |a: u64, b: u64, s: u64, ci: bool| {
            r.put_n(&[3, 1, 19, 17], a);
            r.put_n(&[4, 2, 18, 16], b);
            r.put_n(&[5, 6, 7], s);
            r.put(15, ci);
            r.read_n(&[8, 9, 11, 12]).unwrap()
        };
        for a in 0..16u64 {
            for b in [0u64, 5, 15] {
                assert_eq!(run(a, b, 0, false), 0, "clear");
                assert_eq!(run(a, b, 3, false), (a + b) & 15, "add");
                assert_eq!(run(a, b, 3, true), (a + b + 1) & 15, "add+ci");
                assert_eq!(run(a, b, 4, false), a ^ b, "xor");
                assert_eq!(run(a, b, 5, false), a | b, "or");
                assert_eq!(run(a, b, 6, false), a & b, "and");
                assert_eq!(run(a, b, 7, false), 15, "preset");
                assert_eq!(run(a, b, 2, true), (a.wrapping_sub(b)) & 15, "a-b with carry");
                assert_eq!(run(a, b, 1, true), (b.wrapping_sub(a)) & 15, "b-a with carry");
            }
        }
    }

    #[test]
    fn lookahead_and_true_complement() {
        let mut r = Rig::new(chip_74182());
        // all propagate/generate inactive (active low inputs high): carries follow Cn
        for p in [3, 1, 14, 5, 4, 2, 15, 6] {
            r.put(p, true);
        }
        r.put(13, true);
        assert!(!r.hi(12) && !r.hi(11) && !r.hi(9));
        r.put(4, false); // /P0 active
        assert!(r.hi(12)); // Cn+x = P0 & Cn
        r.put(3, false); // /G0 active
        assert!(r.hi(12) && !r.hi(11));
        let mut r = Rig::new(chip_7487());
        r.put(8, false);
        r.put(1, true);
        r.put(2, true);
        assert!(r.hi(3)); // true
        r.put(1, false);
        assert!(!r.hi(3)); // complement
        r.put(8, true);
        r.put(1, false);
        assert!(r.hi(3)); // B high: !C
    }

    #[test]
    fn addressable_latch_and_register_file() {
        let q = [4u8, 5, 6, 7, 9, 10, 11, 12];
        let mut r = Rig::new(chip_74259());
        r.put(15, true); // MR high
        r.put(14, true); // LE high: hold
        r.put(13, true);
        r.put_n(&[1, 2, 3], 5);
        r.put(14, false); // latch bit 5 = 1
        r.put(14, true);
        assert_eq!(r.read_n(&q), Some(1 << 5));
        r.put_n(&[1, 2, 3], 2);
        r.put(14, false);
        r.put(14, true);
        assert_eq!(r.read_n(&q), Some((1 << 5) | (1 << 2)));
        r.put(15, false); // clear
        assert_eq!(r.read_n(&q), Some(0));

        let mut r = Rig::new(chip_74670());
        r.put(12, true);
        r.put(11, false);
        for w in 0..4u64 {
            r.put_n(&[14, 13], w);
            r.put_n(&[15, 1, 2, 3], w + 5);
            r.put(12, false);
            r.put(12, true);
        }
        for w in 0..4u64 {
            r.put_n(&[5, 4], w);
            assert_eq!(r.read_n(&[10, 9, 7, 6]), Some(w + 5));
        }
        r.put(11, true);
        assert_eq!(r.get(10), Value::UNKNOWN);
    }

    #[test]
    fn monostable_74123() {
        let mut r = Rig::new(chip_74123());
        r.put(3, true); // CLR high
        r.put(2, false);
        r.put(1, false);
        assert!(!r.hi(13) && r.hi(4));
        r.put(2, true); // B rising with A low triggers
        assert!(r.hi(13) && !r.hi(4));
        r.c.tick();
        assert!(r.hi(13)); // 1 tick left of 2
        r.c.tick();
        assert!(!r.hi(13) && r.hi(4));
        r.put(2, true);
        r.put(1, true);
        r.put(1, false); // A falling with B high triggers
        assert!(r.hi(13));
        r.put(3, false); // clear ends pulse
        assert!(!r.hi(13));
    }
}
