//! BFH library (`com.cburch.logisim.std.bfh`): binary → BCD converter and BCD → 7-segment decoder.
//!
//! Port order follows Logisim (not "port 0 = output"), because these parts have an input on
//! port 0 and several equal outputs:
//! * [`BinToBcd`]: port 0 = binary input at (-30, 0); port `i` (1..=digits) = BCD digit of weight
//!   `10^(i-1)` (4 bits) at `((digits - i) * 60, -20)`, i.e. the most significant digit is leftmost.
//! * [`BcdToSevenSeg`]: ports 0..=6 = segments a..g (1 bit each), port 7 = 4-bit BCD input at (10, 80).
//!   Segments are active high and X for invalid input (digits > 9).

use crate::registry::{Library, PartDesc};
use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

/// Registry names are the Logisim ids and the saved part names; never rename them.
pub fn library() -> Library {
    Library {
        name: "BFH",
        parts: vec![
            PartDesc {
                name: "Binary_to_BCD_converter",
                make: || Box::new(BinToBcd::default()),
            },
            PartDesc {
                name: "BCD_to_7_Segment_decoder",
                make: || Box::new(BcdToSevenSeg),
            },
        ],
    }
}

const INNER_DISTANCE: i32 = 60;

/// Binary to BCD converter. Option `bin_bits` (4..=13, default 9): input width.
#[derive(Debug, Clone)]
pub struct BinToBcd {
    bits: u8,
}

impl Default for BinToBcd {
    fn default() -> Self {
        BinToBcd { bits: 9 }
    }
}

impl BinToBcd {
    pub fn new(bits: u8) -> Self {
        BinToBcd {
            bits: bits.clamp(4, 13),
        }
    }
    pub fn bin_bits(mut self, bits: u8) -> Self {
        self.bits = bits.clamp(4, 13);
        self
    }
    /// Number of BCD digit outputs (digits of `2^bits`, as in Logisim).
    pub fn digits(&self) -> usize {
        (1u32 << self.bits).to_string().len()
    }
}

impl Component for BinToBcd {
    fn name(&self) -> &str {
        "Binary_to_BCD_converter"
    }
    fn ports(&self) -> Vec<Port> {
        let n = self.digits() as i32;
        let mut ports = vec![Port::new(
            -INNER_DISTANCE / 2,
            0,
            PortKind::Input,
            BitWidth::new(self.bits),
        )];
        // port i = digit of weight 10^(i-1); the most significant digit is leftmost
        for i in 1..=n {
            ports.push(Port::new(
                (n - i) * INNER_DISTANCE,
                -20,
                PortKind::Output,
                BitWidth::new(4),
            ));
        }
        ports
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("bin_bits", self.bits as i64)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![OptionSpec::int("bin_bits", "Binary input bits", 4, 13, 9)]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["bin_bits"])?;
        if let Some(v) = m.int("bin_bits", 4, 13)? {
            self.bits = v as u8;
        }
        Ok(())
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let input = ctx.port_value(0);
        let digits = self.digits();
        match input.to_u64() {
            Some(mut v) => {
                for i in (1..=digits).rev() {
                    let weight = 10u64.pow(i as u32 - 1);
                    let d = v / weight;
                    ctx.set_port(i, Value::known(BitWidth::new(4), d), 1);
                    v -= d * weight;
                }
            }
            None => {
                let bad = if input.is_error() {
                    Value::error(BitWidth::new(4))
                } else {
                    Value::unknown(BitWidth::new(4))
                };
                for i in 1..=digits {
                    ctx.set_port(i, bad, 1);
                }
            }
        }
    }
}

/// BCD to seven-segment decoder (segments a..g active high).
#[derive(Debug, Clone, Copy, Default)]
pub struct BcdToSevenSeg;

/// Segment patterns for digits 0..=9, bit 0 = a ... bit 6 = g.
const PATTERNS: [u8; 10] = [
    0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
    0b1111111, 0b1101111,
];

impl Component for BcdToSevenSeg {
    fn name(&self) -> &str {
        "BCD_to_7_Segment_decoder"
    }
    fn ports(&self) -> Vec<Port> {
        let one = BitWidth::ONE;
        let out = |x, y| Port::new(x, y, PortKind::Output, one);
        vec![
            out(20, 0),  // a
            out(30, 0),  // b
            out(20, 60), // c
            out(10, 60), // d
            out(0, 60),  // e
            out(10, 0),  // f
            out(0, 0),   // g
            Port::new(10, 80, PortKind::Input, BitWidth::new(4)),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let pattern = ctx
            .port_value(7)
            .to_u64()
            .and_then(|v| PATTERNS.get(v as usize).copied());
        for seg in 0..7 {
            let v = match pattern {
                Some(p) => {
                    if (p >> seg) & 1 == 1 {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                None => Value::UNKNOWN,
            };
            ctx.set_port(seg, v, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiring::Pin;
    use logisim_core::{Circuit, Direction, Location};

    /// Place `part` at (400, 400), feed `input_port` with `v` through a Pin, and return the circuit.
    fn rig(part: Box<dyn Component>, input_port: usize, v: Value) -> (Circuit, logisim_core::CompId) {
        let mut c = Circuit::new();
        let id = c.add_component(Location::new(400, 400), Direction::East, part);
        let loc = c.port_location(id, input_port);
        let far = Location::new(loc.x, loc.y + 40);
        c.add_component(far, Direction::East, Box::new(Pin::new(v)));
        c.add_wire(far, loc);
        c.reset();
        (c, id)
    }

    #[test]
    fn binary_to_bcd() {
        for (bits, v) in [(9u8, 0u64), (9, 7), (9, 499), (9, 511), (4, 15), (13, 8191), (10, 1023)] {
            let part = BinToBcd::new(bits);
            let digits = part.digits();
            let (mut c, id) = rig(Box::new(part), 0, Value::known(BitWidth::new(bits), v));
            let mut got = 0u64;
            for i in (1..=digits).rev() {
                let d = c.port_value(id, i).to_u64().unwrap();
                assert!(d <= 9);
                got = got * 10 + d;
            }
            assert_eq!(got, v, "bits {bits}");
        }
        assert_eq!(BinToBcd::new(9).digits(), 3);
        assert_eq!(BinToBcd::new(4).digits(), 2);
        assert_eq!(BinToBcd::new(13).digits(), 4);
    }

    #[test]
    fn binary_to_bcd_unknown_input() {
        let (mut c, id) = rig(Box::new(BinToBcd::default()), 0, Value::unknown(BitWidth::new(9)));
        assert!(c.port_value(id, 1).is_unknown());
    }

    #[test]
    fn seven_segment() {
        for d in 0..16u64 {
            let (mut c, id) = rig(Box::new(BcdToSevenSeg), 7, Value::known(BitWidth::new(4), d));
            for seg in 0..7 {
                let v = c.port_value(id, seg);
                match PATTERNS.get(d as usize) {
                    Some(p) => assert_eq!(v == Value::TRUE, (p >> seg) & 1 == 1, "digit {d} seg {seg}"),
                    None => assert!(v.is_unknown(), "digit {d}"),
                }
            }
        }
    }

    #[test]
    fn options_round_trip() {
        for p in &library().parts {
            let part = (p.make)();
            assert_eq!(part.name(), p.name);
            let saved = part.options();
            let mut fresh = (p.make)();
            fresh.set_options(&saved).unwrap();
            assert_eq!(fresh.options(), saved);
            assert_eq!(fresh.ports(), part.ports());
        }
        let mut b = BinToBcd::default();
        b.set_options(&OptionMap::new().with("bin_bits", 12)).unwrap();
        assert_eq!(b.digits(), 4);
        assert_eq!(b.ports().len(), 5);
        assert!(b.set_options(&OptionMap::new().with("bin_bits", 3)).is_err());
        assert!(b.set_options(&OptionMap::new().with("nope", 1)).is_err());
    }

    #[test]
    fn option_specs_match_options() {
        for p in &library().parts {
            logisim_core::validate_specs(&mut *(p.make)()).unwrap_or_else(|e| panic!("{e}"));
        }
    }
}
