//! Gate chips: 7400, 7402, 7404, 7405, 7408, 7410, 7411, 7413, 7414, 7418, 7419, 7420, 7421,
//! 7424, 7427, 7430, 7432, 7434, 7436, 7438, 7451, 7454, 7458, 7464, 7486, 74125, 74132.
//!
//! Pin numbers are datasheet pins. Inputs not listed in `outputs` are inputs.

use super::*;

type V2 = fn(Value, Value) -> Value;

fn and(a: Value, b: Value) -> Value {
    a.and(b)
}
fn or(a: Value, b: Value) -> Value {
    a.or(b)
}
fn xor(a: Value, b: Value) -> Value {
    a.xor(b)
}
fn nand(a: Value, b: Value) -> Value {
    !a.and(b)
}
fn nor(a: Value, b: Value) -> Value {
    !a.or(b)
}

/// Quad 2-input gate: `(out, in_a, in_b)` triples.
fn quad(name: &'static str, outs: &'static [u8], gates: &'static [(u8, u8, u8)], f: V2) -> Ttl {
    let spec = Spec::new(name, 14, outs);
    Ttl::new(spec, move |p| {
        for &(o, a, b) in gates {
            let v = f(p.get(a), p.get(b));
            p.set(o, v, 1);
        }
    })
}

const G_7400: &[(u8, u8, u8)] = &[(3, 1, 2), (6, 4, 5), (8, 10, 9), (11, 13, 12)];
const G_7402: &[(u8, u8, u8)] = &[(1, 2, 3), (4, 5, 6), (10, 8, 9), (13, 11, 12)];
const O_7400: &[u8] = &[3, 6, 8, 11];
const O_7402: &[u8] = &[1, 4, 10, 13];

pub fn chip_7400() -> Ttl {
    quad("7400", O_7400, G_7400, nand)
}
pub fn chip_7402() -> Ttl {
    quad("7402", O_7402, G_7402, nor)
}
pub fn chip_7408() -> Ttl {
    quad("7408", O_7400, G_7400, and)
}
pub fn chip_7432() -> Ttl {
    quad("7432", O_7400, G_7400, or)
}
pub fn chip_7436() -> Ttl {
    quad("7436", O_7400, G_7400, nor)
}
pub fn chip_7486() -> Ttl {
    quad("7486", O_7400, G_7400, xor)
}
pub fn chip_7424() -> Ttl {
    quad("7424", O_7400, G_7400, nand)
}
pub fn chip_74132() -> Ttl {
    quad("74132", O_7400, G_7400, nand)
}

/// 7438: quad NAND with open-collector outputs (low or floating).
pub fn chip_7438() -> Ttl {
    let spec = Spec::new("7438", 14, O_7400);
    Ttl::new(spec, |p| {
        for &(o, a, b) in G_7400 {
            let high = !(p.hi(a) && p.hi(b));
            p.set_open(o, high, 1);
        }
    })
}

const HEX: &[(u8, u8)] = &[(2, 1), (4, 3), (6, 5), (8, 9), (10, 11), (12, 13)];
const O_HEX: &[u8] = &[2, 4, 6, 8, 10, 12];

fn hex(name: &'static str, f: fn(Value) -> Value) -> Ttl {
    Ttl::new(Spec::new(name, 14, O_HEX), move |p| {
        for &(o, i) in HEX {
            let v = f(p.get(i));
            p.set(o, v, 1);
        }
    })
}
fn inv(v: Value) -> Value {
    !v
}
fn buf(v: Value) -> Value {
    v
}

pub fn chip_7404() -> Ttl {
    hex("7404", inv)
}
pub fn chip_7414() -> Ttl {
    hex("7414", inv)
}
pub fn chip_7419() -> Ttl {
    hex("7419", inv)
}
pub fn chip_7434() -> Ttl {
    hex("7434", buf)
}
/// 7405: hex inverter, open collector (1 → 0, 0 → floating, X/E → E).
pub fn chip_7405() -> Ttl {
    Ttl::new(Spec::new("7405", 14, O_HEX), |p| {
        for &(o, i) in HEX {
            let v = match p.get(i) {
                Value::TRUE => Value::FALSE,
                Value::FALSE => Value::UNKNOWN,
                _ => Value::ERROR,
            };
            p.set(o, v, 1);
        }
    })
}

/// Triple 3-input gate (7410/7411/7427).
fn triple(name: &'static str, and_gate: bool, inverted: bool) -> Ttl {
    Ttl::new(Spec::new(name, 14, &[6, 8, 12]), move |p| {
        for (o, ins) in [(6u8, [3u8, 4, 5]), (12, [1, 2, 13]), (8, [9, 10, 11])] {
            let (a, b, c) = (p.get(ins[0]), p.get(ins[1]), p.get(ins[2]));
            let v = if and_gate {
                a.and(b.and(c))
            } else {
                a.or(b.or(c))
            };
            p.set(o, if inverted { !v } else { v }, 2);
        }
    })
}
pub fn chip_7410() -> Ttl {
    triple("7410", true, true)
}
pub fn chip_7411() -> Ttl {
    triple("7411", true, false)
}
pub fn chip_7427() -> Ttl {
    triple("7427", false, true)
}

/// Dual 4-input AND/NAND (7413, 7418, 7420, 7421).
fn dual4(name: &'static str, inverted: bool) -> Ttl {
    let spec = Spec::new(name, 14, &[6, 8]).unused(&[3, 11]);
    Ttl::new(spec, move |p| {
        for (o, ins, d) in [(6u8, [1u8, 2, 4, 5], 3u32), (8, [9, 10, 12, 13], 4)] {
            let v = ins
                .iter()
                .map(|&i| p.get(i))
                .reduce(|a, b| a.and(b))
                .unwrap();
            p.set(o, if inverted { !v } else { v }, d);
        }
    })
}
pub fn chip_7413() -> Ttl {
    dual4("7413", true)
}
pub fn chip_7418() -> Ttl {
    dual4("7418", true)
}
pub fn chip_7420() -> Ttl {
    dual4("7420", true)
}
pub fn chip_7421() -> Ttl {
    dual4("7421", false)
}

/// 7430: 8-input NAND.
pub fn chip_7430() -> Ttl {
    let spec = Spec::new("7430", 14, &[8]).unused(&[9, 10, 13]);
    Ttl::new(spec, |p| {
        let v = [1u8, 2, 3, 4, 5, 6, 11, 12]
            .iter()
            .map(|&i| p.get(i))
            .reduce(|a, b| a.and(b))
            .unwrap();
        p.set(8, !v, 1);
    })
}

/// 7451: dual AND-OR-INVERT (2-wide AND / 2-input).
pub fn chip_7451() -> Ttl {
    let spec = Spec::new("7451", 14, &[6, 8]).unused(&[11, 12]);
    Ttl::new(spec, |p| {
        let v = p.get(2).and(p.get(3)).or(p.get(4).and(p.get(5)));
        p.set(6, !v, 3);
        let v = p.get(1).and(p.get(13)).or(p.get(9).and(p.get(10)));
        p.set(8, !v, 3);
    })
}

/// 7454: 4-wide 2-input AND-OR-INVERT.
pub fn chip_7454() -> Ttl {
    let spec = Spec::new("7454", 14, &[8]).unused(&[6, 11, 12]);
    Ttl::new(spec, |p| {
        let v = p
            .get(1)
            .and(p.get(13))
            .or(p.get(2).and(p.get(3)))
            .or(p.get(4).and(p.get(5)))
            .or(p.get(9).and(p.get(10)));
        p.set(8, !v, 3);
    })
}

/// 7458: dual AND-OR (2×3 + 2×2 wide), not inverted.
pub fn chip_7458() -> Ttl {
    let spec = Spec::new("7458", 14, &[6, 8]);
    Ttl::new(spec, |p| {
        let v = p.get(2).and(p.get(3)).or(p.get(4).and(p.get(5)));
        p.set(6, v, 5);
        let a = p.get(1).and(p.get(13).and(p.get(12)));
        let b = p.get(11).and(p.get(10).and(p.get(9)));
        p.set(8, a.or(b), 5);
    })
}

/// 7464: 4-2-3-2 AND-OR-INVERT.
pub fn chip_7464() -> Ttl {
    let spec = Spec::new("7464", 14, &[8]).height(70);
    Ttl::new(spec, |p| {
        let v1 = p.get(2).and(p.get(3));
        let v2 = p.get(4).and(p.get(5).and(p.get(6)));
        let v3 = p.get(9).and(p.get(10));
        let v4 = p.get(11).and(p.get(12).and(p.get(13).and(p.get(1))));
        p.set(8, !v1.or(v2.or(v3.or(v4))), 7);
    })
}

/// 74125: quad bus buffer, enable active LOW (disabled → X).
pub fn chip_74125() -> Ttl {
    Ttl::new(Spec::new("74125", 14, O_7400), |p| {
        for (en, i, o) in [(1u8, 2u8, 3u8), (4, 5, 6), (10, 9, 8), (13, 12, 11)] {
            let v = if p.hi(en) { Value::UNKNOWN } else { p.get(i) };
            p.set(o, v, 1);
        }
    })
}

pub fn parts() -> Vec<PartDesc> {
    chips! {
        "7400" => chip_7400, "7402" => chip_7402, "7404" => chip_7404, "7405" => chip_7405,
        "7408" => chip_7408, "7410" => chip_7410, "7411" => chip_7411, "7413" => chip_7413,
        "7414" => chip_7414, "7418" => chip_7418, "7419" => chip_7419, "7420" => chip_7420,
        "7421" => chip_7421, "7424" => chip_7424, "7427" => chip_7427, "7430" => chip_7430,
        "7432" => chip_7432, "7434" => chip_7434, "7436" => chip_7436, "7438" => chip_7438,
        "7451" => chip_7451, "7454" => chip_7454, "7458" => chip_7458, "7464" => chip_7464,
        "7486" => chip_7486, "74125" => chip_74125, "74132" => chip_74132,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::Rig;
    use super::*;

    fn table2(make: fn() -> Ttl, gates: &[(u8, u8, u8)], f: fn(bool, bool) -> bool) {
        for &(o, a, b) in gates {
            let mut r = Rig::new(make());
            for i in 0..4 {
                let (x, y) = (i & 1 == 1, i & 2 == 2);
                r.put(a, x);
                r.put(b, y);
                assert_eq!(r.hi(o), f(x, y), "pin {o} for {x} {y}");
            }
        }
    }

    #[test]
    fn two_input_gates() {
        table2(chip_7400, G_7400, |a, b| !(a && b));
        table2(chip_7402, G_7402, |a, b| !(a || b));
        table2(chip_7408, G_7400, |a, b| a && b);
        table2(chip_7432, G_7400, |a, b| a || b);
        table2(chip_7436, G_7400, |a, b| !(a || b));
        table2(chip_7486, G_7400, |a, b| a ^ b);
        table2(chip_74132, G_7400, |a, b| !(a && b));
    }

    #[test]
    fn open_collector_nand_floats_high() {
        let mut r = Rig::new(chip_7438());
        r.put(1, true);
        r.put(2, true);
        assert_eq!(r.get(3), Value::FALSE);
        r.put(2, false);
        assert_eq!(r.get(3), Value::UNKNOWN);
    }

    #[test]
    fn inverters_and_open_drain() {
        let mut r = Rig::new(chip_7404());
        for &(o, i) in HEX {
            r.put(i, true);
            assert_eq!(r.get(o), Value::FALSE);
            r.put(i, false);
            assert_eq!(r.get(o), Value::TRUE);
        }
        let mut r = Rig::new(chip_7405());
        r.put(13, true);
        assert_eq!(r.get(12), Value::FALSE);
        r.put(13, false);
        assert_eq!(r.get(12), Value::UNKNOWN);
        r.set(13, Value::UNKNOWN);
        assert_eq!(r.get(12), Value::ERROR);
    }

    #[test]
    fn triples_and_quads() {
        let mut r = Rig::new(chip_7410());
        for (o, ins) in [(6u8, [3u8, 4, 5]), (12, [1, 2, 13]), (8, [9, 10, 11])] {
            for i in 0..8 {
                for (k, &p) in ins.iter().enumerate() {
                    r.put(p, (i >> k) & 1 == 1);
                }
                assert_eq!(r.hi(o), i != 7);
            }
        }
        let mut r = Rig::new(chip_7427());
        for i in 0..8 {
            for (k, &p) in [3u8, 4, 5].iter().enumerate() {
                r.put(p, (i >> k) & 1 == 1);
            }
            assert_eq!(r.hi(6), i == 0);
        }
        let mut r = Rig::new(chip_7420());
        for i in 0..16 {
            for (k, &p) in [9u8, 10, 12, 13].iter().enumerate() {
                r.put(p, (i >> k) & 1 == 1);
            }
            assert_eq!(r.hi(8), i != 15);
        }
        let mut r = Rig::new(chip_7430());
        for i in 0..256u32 {
            for (k, &p) in [1u8, 2, 3, 4, 5, 6, 11, 12].iter().enumerate() {
                r.put(p, (i >> k) & 1 == 1);
            }
            assert_eq!(r.hi(8), i != 255);
        }
    }

    #[test]
    fn and_or_invert() {
        let mut r = Rig::new(chip_7454());
        r.put(1, true);
        r.put(13, true);
        assert!(!r.hi(8));
        r.put(1, false);
        assert!(r.hi(8));
        r.put(9, true);
        r.put(10, true);
        assert!(!r.hi(8));
        let mut r = Rig::new(chip_7458());
        r.put(2, true);
        r.put(3, true);
        assert!(r.hi(6));
        assert!(!r.hi(8));
        for p in [1, 13, 12] {
            r.put(p, true);
        }
        assert!(r.hi(8));
        let mut r = Rig::new(chip_7464());
        assert!(r.hi(8));
        for p in [11, 12, 13, 1] {
            r.put(p, true);
        }
        assert!(!r.hi(8));
        let mut r = Rig::new(chip_7451());
        assert!(r.hi(6) && r.hi(8));
        r.put(4, true);
        r.put(5, true);
        assert!(!r.hi(6) && r.hi(8));
    }

    #[test]
    fn bus_buffer_tristate() {
        let mut r = Rig::new(chip_74125());
        r.put(2, true);
        assert!(r.hi(3));
        r.put(1, true);
        assert_eq!(r.get(3), Value::UNKNOWN);
    }

    #[test]
    fn power_pins_option() {
        let chip = Ttl::with_options(
            Spec::new("7400", 14, O_7400),
            TtlOptions::default().vcc_gnd_ports(true),
            |p| p.set(3, Value::TRUE, 1),
        );
        assert_eq!(chip.ports().len(), 14); // 12 signal + GND + VCC
        let mut r = Rig::new(chip);
        assert_eq!(r.get(3), Value::UNKNOWN); // GND/VCC not applied
        r.put(14, true);
        assert_eq!(r.get(3), Value::TRUE);
        r.put(7, true);
        assert_eq!(r.get(3), Value::UNKNOWN);
    }
}
