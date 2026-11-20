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

pub fn parts() -> Vec<PartDesc> {
    chips! {
        "74173" => chip_74173, "74175" => chip_74175, "74273" => chip_74273,
        "74374" => chip_74374, "74377" => chip_74377,
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
}
