//! Decoders, encoders and multiplexers: 7442, 7443, 7444, 74138, 74139, 74148, 74151, 74153,
//! 74157, 74158.

use super::*;

/// BCD-ish decoder core shared by 7442/7443/7444: active-low outputs Y0..Y9.
fn decoder_10(name: &'static str, map: fn(u8) -> Option<usize>) -> Ttl {
    const OUT: [u8; 10] = [1, 2, 3, 4, 5, 6, 7, 9, 10, 11];
    let spec = Spec::new(name, 16, &OUT);
    Ttl::new(spec, move |p| {
        // A=15 (LSB), B=14, C=13, D=12
        match p.read(&[15, 14, 13, 12]) {
            None => p.write_unknown(&OUT, 1),
            Some(v) => {
                let hit = map(v as u8);
                for (i, &o) in OUT.iter().enumerate() {
                    p.set_bool(o, hit != Some(i), 1);
                }
            }
        }
    })
}

/// 7442: BCD to decimal.
pub fn chip_7442() -> Ttl {
    decoder_10("7442", |v| Some(v as usize))
}
/// 7443: excess-3 to decimal.
pub fn chip_7443() -> Ttl {
    decoder_10("7443", |v| (v as usize).checked_sub(3))
}
/// 7444: excess-3 Gray to decimal.
pub fn chip_7444() -> Ttl {
    decoder_10("7444", |v| {
        [2u8, 6, 7, 5, 4, 12, 13, 15, 14, 10]
            .iter()
            .position(|&g| g == v)
    })
}

/// 74138: 3-to-8 decoder, active-low outputs.
pub fn chip_74138() -> Ttl {
    const OUT: [u8; 8] = [15, 14, 13, 12, 11, 10, 9, 7];
    Ttl::new(Spec::new("74138", 16, &OUT), |p| {
        let enabled = p.hi(6) && p.lo(4) && p.lo(5);
        let sel = (p.hi(1) as usize) | (p.hi(2) as usize) << 1 | (p.hi(3) as usize) << 2;
        for (i, &o) in OUT.iter().enumerate() {
            p.set_bool(o, !(enabled && sel == i), 1);
        }
    })
}

/// 74139: dual 2-to-4 decoder, active-low enable and outputs.
pub fn chip_74139() -> Ttl {
    const OUT: [u8; 8] = [4, 5, 6, 7, 12, 11, 10, 9];
    Ttl::new(Spec::new("74139", 16, &OUT), |p| {
        for (en, a, b, outs) in [(1u8, 2u8, 3u8, &OUT[..4]), (15, 14, 13, &OUT[4..])] {
            let enabled = p.lo(en);
            let sel = (p.hi(a) as usize) | (p.hi(b) as usize) << 1;
            for (i, &o) in outs.iter().enumerate() {
                p.set_bool(o, !(enabled && sel == i), 1);
            }
        }
    })
}

/// 74148: 8-to-3 priority encoder, all pins active low.
pub fn chip_74148() -> Ttl {
    const IN: [u8; 8] = [10, 11, 12, 13, 1, 2, 3, 4];
    Ttl::new(Spec::new("74148", 16, &[6, 7, 9, 14, 15]), |p| {
        let enabled = p.lo(5);
        let code = if enabled {
            (0..8).rev().find(|&c| p.lo(IN[c]))
        } else {
            None
        };
        let enc = code.is_some();
        let c = code.unwrap_or(0);
        p.set_bool(9, !(enc && c & 1 != 0), 1);
        p.set_bool(7, !(enc && c & 2 != 0), 1);
        p.set_bool(6, !(enc && c & 4 != 0), 1);
        p.set_bool(14, !enc, 1);
        p.set_bool(15, !(enabled && !enc), 1);
    })
}

/// 74151: 8-to-1 multiplexer with strobe (active low) and complementary outputs.
pub fn chip_74151() -> Ttl {
    const D: [u8; 8] = [4, 3, 2, 1, 15, 14, 13, 12];
    Ttl::new(Spec::new("74151", 16, &[5, 6]), |p| {
        let sel = p.read(&[11, 10, 9]).unwrap_or_else(|| {
            // Java treats anything but a clean 1 as 0.
            (p.hi(11) as u64) | (p.hi(10) as u64) << 1 | (p.hi(9) as u64) << 2
        }) as usize;
        let d = p.hi(D[sel]);
        let g = p.hi(7);
        p.set_bool(5, !g && d, 1);
        p.set_bool(6, g || !d, 1);
    })
}

/// 74153: dual 4-to-1 multiplexer.
pub fn chip_74153() -> Ttl {
    Ttl::new(Spec::new("74153", 16, &[7, 9]).height(80), |p| {
        let sel = (p.hi(14) as usize) | (p.hi(2) as usize) << 1;
        let d1 = [6u8, 5, 4, 3][sel];
        let d2 = [10u8, 11, 12, 13][sel];
        let y1 = !p.hi(1) && p.hi(d1);
        let y2 = !p.hi(15) && p.hi(d2);
        p.set_bool(7, y1, 1);
        p.set_bool(9, y2, 1);
    })
}

fn quad_2to1(name: &'static str, invert: bool) -> Ttl {
    Ttl::new(Spec::new(name, 16, &[4, 7, 9, 12]), move |p| {
        let strobe = p.hi(15);
        let select = p.hi(1);
        for (a, b, y) in [(2u8, 3u8, 4u8), (5, 6, 7), (11, 10, 9), (14, 13, 12)] {
            let mut v = !strobe && if select { p.hi(b) } else { p.hi(a) };
            if invert {
                v = !v;
            }
            p.set_bool(y, v, 1);
        }
    })
}
/// 74157: quad 2-to-1 multiplexer.
pub fn chip_74157() -> Ttl {
    quad_2to1("74157", false)
}
/// 74158: quad 2-to-1 multiplexer, inverting outputs.
pub fn chip_74158() -> Ttl {
    quad_2to1("74158", true)
}

pub fn parts() -> Vec<PartDesc> {
    chips! {
        "7442" => chip_7442, "7443" => chip_7443, "7444" => chip_7444,
        "74138" => chip_74138, "74139" => chip_74139, "74148" => chip_74148,
        "74151" => chip_74151, "74153" => chip_74153, "74157" => chip_74157,
        "74158" => chip_74158,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::Rig;
    use super::*;

    #[test]
    fn bcd_decoders() {
        const OUT: [u8; 10] = [1, 2, 3, 4, 5, 6, 7, 9, 10, 11];
        let sel = [15u8, 14, 13, 12];
        let mut r = Rig::new(chip_7442());
        for v in 0..16u64 {
            r.put_n(&sel, v);
            for (i, &o) in OUT.iter().enumerate() {
                assert_eq!(r.hi(o), v as usize != i, "7442 in {v} out {i}");
            }
        }
        let mut r = Rig::new(chip_7443());
        for v in 0..16u64 {
            r.put_n(&sel, v);
            for (i, &o) in OUT.iter().enumerate() {
                assert_eq!(r.hi(o), v as usize != i + 3, "7443 in {v} out {i}");
            }
        }
        let mut r = Rig::new(chip_7444());
        let gray = [2u64, 6, 7, 5, 4, 12, 13, 15, 14, 10];
        for (digit, &g) in gray.iter().enumerate() {
            r.put_n(&sel, g);
            for (i, &o) in OUT.iter().enumerate() {
                assert_eq!(r.hi(o), digit != i);
            }
        }
        r.put_n(&sel, 0);
        assert!(OUT.iter().all(|&o| r.hi(o)));
    }

    #[test]
    fn decoder_unknown_input_floats_outputs() {
        let mut r = Rig::new(chip_7442());
        r.set(15, Value::UNKNOWN);
        assert_eq!(r.get(1), Value::UNKNOWN);
    }

    #[test]
    fn three_to_eight() {
        const OUT: [u8; 8] = [15, 14, 13, 12, 11, 10, 9, 7];
        let mut r = Rig::new(chip_74138());
        r.put(6, true);
        for v in 0..8u64 {
            r.put_n(&[1, 2, 3], v);
            for (i, &o) in OUT.iter().enumerate() {
                assert_eq!(r.hi(o), v as usize != i);
            }
        }
        r.put(4, true); // disable
        assert!(OUT.iter().all(|&o| r.hi(o)));
        r.put(4, false);
        r.put(6, false);
        assert!(OUT.iter().all(|&o| r.hi(o)));
    }

    #[test]
    fn dual_two_to_four() {
        let mut r = Rig::new(chip_74139());
        for v in 0..4u64 {
            r.put_n(&[2, 3], v);
            r.put_n(&[14, 13], v);
            for i in 0..4usize {
                assert_eq!(r.hi([4, 5, 6, 7][i]), v as usize != i);
                assert_eq!(r.hi([12, 11, 10, 9][i]), v as usize != i);
            }
        }
        r.put(1, true);
        assert!([4, 5, 6, 7].iter().all(|&o| r.hi(o)));
        assert!(!r.hi(12 - 3)); // second half still enabled: Y3 low at v=3
    }

    #[test]
    fn priority_encoder() {
        const IN: [u8; 8] = [10, 11, 12, 13, 1, 2, 3, 4];
        let mut r = Rig::new(chip_74148());
        for &i in IN.iter().chain([5u8].iter()) {
            r.put(i, true);
        }
        r.put(5, false); // enable
        // none asserted: outputs high, GS high, EO low
        assert!(r.hi(6) && r.hi(7) && r.hi(9) && r.hi(14));
        assert!(!r.hi(15));
        r.put(IN[2], false);
        assert_eq!(r.read_n(&[9, 7, 6]), Some(0b101)); // ~2 = 101
        assert!(!r.hi(14) && r.hi(15));
        r.put(IN[6], false);
        assert_eq!(r.read_n(&[9, 7, 6]), Some(0b001)); // ~6 = 001
        r.put(5, true);
        assert!(r.hi(14) && r.hi(15));
    }

    #[test]
    fn multiplexers() {
        const D: [u8; 8] = [4, 3, 2, 1, 15, 14, 13, 12];
        let mut r = Rig::new(chip_74151());
        for (s, &sel_pin) in D.iter().enumerate() {
            r.put_n(&[11, 10, 9], s as u64);
            for (k, &d) in D.iter().enumerate() {
                r.put(d, k == s);
            }
            assert!(r.hi(5) && !r.hi(6));
            r.put(sel_pin, false);
            assert!(!r.hi(5) && r.hi(6));
        }
        r.put(7, true);
        assert!(!r.hi(5) && r.hi(6));

        let mut r = Rig::new(chip_74153());
        r.put(6, true);
        r.put(11, true);
        assert!(r.hi(7) && !r.hi(9));
        r.put_n(&[14, 2], 1);
        assert!(!r.hi(7) && r.hi(9));
        r.put(1, true);
        assert!(!r.hi(7));

        let mut r = Rig::new(chip_74157());
        r.put(2, true);
        r.put(3, false);
        assert!(r.hi(4));
        r.put(1, true);
        assert!(!r.hi(4));
        r.put(15, true);
        assert!(!r.hi(4));
        let mut r = Rig::new(chip_74158());
        assert!(r.hi(4));
        r.put(15, true);
        assert!(r.hi(4));
    }
}
