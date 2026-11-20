//! Bus buffers and transceivers: 74240, 74241, 74244, 74245, 74365, 74541, 74640.

use super::*;

const OCTAL_OUT: [u8; 8] = [18, 16, 14, 12, 9, 7, 5, 3];

/// Octal buffer, two 4-bit channels. Channel 1: enable pin 1, A pins 2,4,6,8, Y pins 18,16,14,12.
/// Channel 2: enable pin 19, A pins 11,13,15,17, Y pins 9,7,5,3.
fn octal(name: &'static str, invert: [bool; 2], enable_high: [bool; 2]) -> Ttl {
    let spec = Spec::new(name, 20, &OCTAL_OUT);
    Ttl::new(spec, move |p| {
        let chans = [
            (1u8, [(18u8, 2u8), (16, 4), (14, 6), (12, 8)]),
            (19, [(9, 11), (7, 13), (5, 15), (3, 17)]),
        ];
        for (c, (en, pairs)) in chans.iter().enumerate() {
            let disabled = p.get(*en) == bit(!enable_high[c]);
            for &(y, a) in pairs {
                let v = if disabled {
                    Value::UNKNOWN
                } else if invert[c] {
                    !p.get(a)
                } else {
                    p.get(a)
                };
                p.set(y, v, 1);
            }
        }
    })
}
/// 74240: octal inverting buffer, both enables active low.
pub fn chip_74240() -> Ttl {
    octal("74240", [true, true], [false, false])
}
/// 74241: octal buffer, 1G active low, 2G active high.
pub fn chip_74241() -> Ttl {
    octal("74241", [false, false], [false, true])
}
/// 74244: octal buffer, both enables active low.
pub fn chip_74244() -> Ttl {
    octal("74244", [false, false], [false, false])
}

/// Octal bus transceiver: A1..A8 = pins 2..9, B1..B8 = pins 18..11, DIR = 1 (high: A→B),
/// nOE = 19. Released side floats (X).
fn transceiver(name: &'static str, invert: bool) -> Ttl {
    let io: &'static [u8] = &[2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18];
    let spec = Spec::new(name, 20, &[]).inouts(io);
    Ttl::new(spec, move |p| {
        let a = |k: u8| 1 + k; // k = 1..=8
        let b = |k: u8| 19 - k;
        let dir = p.get(1);
        let enabled = p.lo(19);
        let (src_dir_ab, src_dir_ba) = (enabled && dir == Value::TRUE, enabled && dir == Value::FALSE);
        for k in 1..=8u8 {
            let f = |v: Value| if invert { !v } else { v };
            let (av, bv) = (p.get(a(k)), p.get(b(k)));
            p.set(b(k), if src_dir_ab { f(av) } else { Value::UNKNOWN }, 1);
            p.set(a(k), if src_dir_ba { f(bv) } else { Value::UNKNOWN }, 1);
        }
    })
}
/// 74245: octal bus transceiver.
pub fn chip_74245() -> Ttl {
    transceiver("74245", false)
}
/// 74640: octal inverting bus transceiver.
pub fn chip_74640() -> Ttl {
    transceiver("74640", true)
}

/// 74365: hex buffer, two active-low enables (both must be low).
pub fn chip_74365() -> Ttl {
    Ttl::new(Spec::new("74365", 16, &[3, 5, 7, 9, 11, 13]), |p| {
        let on = p.lo(1) && p.lo(15);
        for (a, y) in [(2u8, 3u8), (4, 5), (6, 7), (10, 9), (12, 11), (14, 13)] {
            let v = if on { p.get(a) } else { Value::UNKNOWN };
            p.set(y, v, 1);
        }
    })
}

/// 74541: octal buffer, A1..A8 (pins 2..9) → Y1..Y8 (pins 18..11), two active-low enables.
pub fn chip_74541() -> Ttl {
    Ttl::new(Spec::new("74541", 20, &[18, 17, 16, 15, 14, 13, 12, 11]), |p| {
        let on = p.lo(1) && p.lo(19);
        for i in 0..8u8 {
            let v = if on { p.get(2 + i) } else { Value::UNKNOWN };
            p.set(18 - i, v, 1);
        }
    })
}

pub fn parts() -> Vec<PartDesc> {
    chips! {
        "74240" => chip_74240, "74241" => chip_74241, "74244" => chip_74244,
        "74245" => chip_74245, "74365" => chip_74365, "74541" => chip_74541,
        "74640" => chip_74640,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::Rig;
    use super::*;

    #[test]
    fn octal_buffers() {
        // 74244: pass when enable low, float otherwise
        let mut r = Rig::new(chip_74244());
        r.put(2, true);
        r.put(17, true);
        assert!(r.hi(18));
        assert!(r.hi(3));
        r.put(1, true);
        assert_eq!(r.get(18), Value::UNKNOWN);
        assert!(r.hi(3)); // channel 2 independent
        r.put(19, true);
        assert_eq!(r.get(3), Value::UNKNOWN);
        // 74240 inverts
        let mut r = Rig::new(chip_74240());
        r.put(2, true);
        assert!(!r.hi(18));
        // 74241: channel 2 enable is active high
        let mut r = Rig::new(chip_74241());
        r.put(11, true);
        assert_eq!(r.get(9), Value::UNKNOWN);
        r.put(19, true);
        assert!(r.hi(9));
    }

    #[test]
    fn transceivers() {
        let release = |r: &mut Rig| {
            for p in [2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18] {
                r.set(p, Value::UNKNOWN);
            }
        };
        let mut r = Rig::new(chip_74245());
        release(&mut r);
        r.put(19, false);
        r.put(1, true); // A -> B
        r.put(5, true); // A4
        assert!(r.hi(15)); // B4 = pin 19-4
        r.put(19, true); // disabled
        assert_eq!(r.get(15), Value::UNKNOWN);
        r.put(19, false);
        r.put(1, false); // B -> A
        r.set(5, Value::UNKNOWN);
        r.put(15, true);
        assert!(r.hi(5));
        let mut r = Rig::new(chip_74640());
        release(&mut r);
        r.put(19, false);
        r.put(1, true);
        r.put(5, true);
        assert_eq!(r.get(15), Value::FALSE); // inverting
    }

    #[test]
    fn hex_and_octal_enable() {
        let mut r = Rig::new(chip_74365());
        r.put(2, true);
        assert!(r.hi(3));
        r.put(15, true);
        assert_eq!(r.get(3), Value::UNKNOWN);
        let mut r = Rig::new(chip_74541());
        r.put(9, true); // A8 -> Y8 = pin 11
        assert!(r.hi(11));
        r.put(19, true);
        assert_eq!(r.get(11), Value::UNKNOWN);
    }
}
