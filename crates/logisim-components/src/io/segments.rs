//! 7-segment display and hex-digit display. Both record a segment mask for the UI.
//!
//! Mask bits: A=1, B=2, C=4, D=8, E=16, F=32, G=64, DP=128.
//!
//! 7-Segment ports (separate pins): A(20,0) B(30,0) C(20,60) D(10,60) E(0,60) F(10,0) G(0,0),
//! then DP(30,60) if enabled — port index = segment number. In bus mode there is one input
//! (7 or 8 bits, bit i = segment i) at the `bus_side` edge.
//! Hex Digit ports: 0 = 4-bit digit at (0,0); 1 = DP at (20,0) if enabled.

use std::cell::Cell;
use std::rc::Rc;

use logisim_core::{BitWidth, Component, Direction, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

/// Segments that are currently driven high (see module docs for the bit layout).
pub type SegmentHandle = Rc<Cell<u8>>;

/// Segments that are lit, given the raw "driven high" mask. Active-low displays light segments
/// whose input is *not* high.
pub fn lit_mask(raw: u8, active_high: bool, with_dp: bool) -> u8 {
    let all = if with_dp { 0xFF } else { 0x7F };
    if active_high {
        raw & all
    } else {
        !raw & all
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SevenSegmentInput {
    /// One pin per segment (default).
    #[default]
    Separate,
    /// A single 7/8-bit bus.
    Bus,
}

const INPUT_MODES: [(&str, SevenSegmentInput); 2] = [("separate", SevenSegmentInput::Separate), ("bus", SevenSegmentInput::Bus)];
const SIDES: [(&str, Direction); 4] =
    [("north", Direction::North), ("south", Direction::South), ("east", Direction::East), ("west", Direction::West)];

pub struct SevenSegment {
    active_high: bool,
    decimal_point: bool,
    input: SevenSegmentInput,
    bus_side: Direction,
    seen: SegmentHandle,
}

impl SevenSegment {
    pub fn new() -> Self {
        SevenSegment {
            active_high: true,
            decimal_point: false,
            input: SevenSegmentInput::Separate,
            bus_side: Direction::West,
            seen: Rc::new(Cell::new(0)),
        }
    }
    pub fn handle(&self) -> SegmentHandle {
        Rc::clone(&self.seen)
    }
    pub fn active_high(&self) -> bool {
        self.active_high
    }
    fn segments(&self) -> usize {
        if self.decimal_point {
            8
        } else {
            7
        }
    }
    fn bus_port_at(&self) -> (i32, i32) {
        // Body spans x -5..35, y 0..60; bus pins sit on the nearest grid point.
        match self.bus_side {
            Direction::North => (10, 0),
            Direction::South => (10, 60),
            Direction::East => (40, 30),
            Direction::West => (-10, 30),
        }
    }
}

impl Default for SevenSegment {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for SevenSegment {
    fn name(&self) -> &str {
        "7-Segment Display"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new()
            .with("active_high", self.active_high)
            .with("decimal_point", self.decimal_point)
            .with("input", OptionMap::name_of(self.input, &INPUT_MODES))
            .with("bus_side", OptionMap::name_of(self.bus_side, &SIDES))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::boolean("active_high", "Segment lit when input is high", true),
            OptionSpec::boolean("decimal_point", "Decimal point", false),
            OptionSpec::choice("input", "Input format", &["separate", "bus"], "separate"),
            OptionSpec::choice("bus_side", "Bus side", &["north", "south", "east", "west"], "west"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["active_high", "decimal_point", "input", "bus_side"])?;
        self.active_high = m.boolean("active_high")?.unwrap_or(self.active_high);
        self.decimal_point = m.boolean("decimal_point")?.unwrap_or(self.decimal_point);
        self.input = m.choice("input", &INPUT_MODES)?.unwrap_or(self.input);
        self.bus_side = m.choice("bus_side", &SIDES)?.unwrap_or(self.bus_side);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        if self.input == SevenSegmentInput::Bus {
            let (x, y) = self.bus_port_at();
            return vec![Port::new(x, y, PortKind::Input, BitWidth::new(self.segments() as u8))];
        }
        let mut ps: Vec<(i32, i32)> = vec![(20, 0), (30, 0), (20, 60), (10, 60), (0, 60), (10, 0), (0, 0)];
        if self.decimal_point {
            ps.push((30, 60));
        }
        ps.into_iter().map(|(x, y)| Port::new(x, y, PortKind::Input, BitWidth::ONE)).collect()
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let n = self.segments();
        let mut mask = 0u8;
        for i in 0..n {
            let bit = match self.input {
                SevenSegmentInput::Bus => ctx.port_value(0).bit(i as u8),
                SevenSegmentInput::Separate => ctx.port_value(i),
            };
            if bit == Value::TRUE {
                mask |= 1 << i;
            }
        }
        self.seen.set(mask);
    }
}

// Logisim stores each digit's segments as a hex literal with one 0/1 nibble per segment.
const SEG_A: u32 = 0x0010000;
const SEG_B: u32 = 0x0000010;
const SEG_C: u32 = 0x0000001;
const SEG_D: u32 = 0x0000100;
const SEG_E: u32 = 0x0100000;
const SEG_F: u32 = 0x1000000;
const SEG_G: u32 = 0x0001000;

const HEX_SEGS: [u32; 16] = [
    0x1110111, 0x0000011, 0x0111110, 0x0011111, 0x1001011, 0x1011101, 0x1111101, 0x0010011, 0x1111111, 0x1011011, 0x1111011,
    0x1101101, 0x1110100, 0x0101111, 0x1111100, 0x1111000,
];

/// Segment mask (A=1..G=64) for a hex digit; 0 (blank) if `digit` is `None`.
pub fn hex_segments(digit: Option<u64>) -> u8 {
    let segs = match digit {
        Some(d) if d < 16 => HEX_SEGS[d as usize],
        _ => 0,
    };
    let mut m = 0;
    for (mask, bit) in [(SEG_A, 1), (SEG_B, 2), (SEG_C, 4), (SEG_D, 8), (SEG_E, 16), (SEG_F, 32), (SEG_G, 64)] {
        if segs & mask != 0 {
            m |= bit;
        }
    }
    m
}

pub struct HexDigit {
    decimal_point: bool,
    seen: SegmentHandle,
}

impl HexDigit {
    pub fn new() -> Self {
        HexDigit { decimal_point: false, seen: Rc::new(Cell::new(0)) }
    }
    pub fn handle(&self) -> SegmentHandle {
        Rc::clone(&self.seen)
    }
}

impl Default for HexDigit {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for HexDigit {
    fn name(&self) -> &str {
        "Hex Digit Display"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("decimal_point", self.decimal_point)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::boolean("decimal_point", "Decimal point", false),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["decimal_point"])?;
        self.decimal_point = m.boolean("decimal_point")?.unwrap_or(self.decimal_point);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let mut ps = vec![Port::new(0, 0, PortKind::Input, BitWidth::new(4))];
        if self.decimal_point {
            ps.push(Port::new(20, 0, PortKind::Input, BitWidth::ONE));
        }
        ps
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let mut mask = hex_segments(ctx.port_value(0).to_u64());
        if self.decimal_point && ctx.port_value(1) == Value::TRUE {
            mask |= 128;
        }
        self.seen.set(mask);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::testutil::Rig;

    #[test]
    fn hex_digit_patterns() {
        // classic 7-segment encodings, bit0=A .. bit6=G (Logisim draws 9 without the bottom bar)
        let expect: [u8; 16] = [
            0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x67, 0x77, 0x7C, 0x39, 0x5E, 0x79, 0x71,
        ];
        for (d, want) in expect.into_iter().enumerate() {
            assert_eq!(hex_segments(Some(d as u64)), want, "digit {d:X}");
        }
        assert_eq!(hex_segments(None), 0);
    }

    #[test]
    fn hex_digit_component() {
        let h = HexDigit::new();
        let seen = h.handle();
        let mut r = Rig::new(Box::new(h), &[(0, Value::known(BitWidth::new(4), 0xA))]);
        assert_eq!(seen.get(), 0x77);
        r.set(0, Value::unknown(BitWidth::new(4)));
        assert_eq!(seen.get(), 0);
    }

    #[test]
    fn hex_digit_decimal_point() {
        let mut h = HexDigit::new();
        h.set_options(&OptionMap::new().with("decimal_point", true)).unwrap();
        let seen = h.handle();
        let mut r = Rig::new(Box::new(h), &[(0, Value::known(BitWidth::new(4), 1)), (1, Value::TRUE)]);
        assert_eq!(seen.get(), 0x06 | 128);
        r.set(1, Value::FALSE);
        assert_eq!(seen.get(), 0x06);
    }

    #[test]
    fn seven_segment_separate_pins() {
        let s = SevenSegment::new();
        let seen = s.handle();
        let ins: Vec<_> = (0..7).map(|i| (i, if i == 1 || i == 6 { Value::TRUE } else { Value::FALSE })).collect();
        let mut r = Rig::new(Box::new(s), &ins);
        assert_eq!(seen.get(), 0b100_0010);
        r.set(0, Value::TRUE);
        assert_eq!(seen.get(), 0b100_0011);
        assert_eq!(lit_mask(seen.get(), false, false), 0b011_1100);
    }

    #[test]
    fn seven_segment_bus_with_dp() {
        let mut s = SevenSegment::new();
        s.set_options(&OptionMap::new().with("input", "bus").with("decimal_point", true).with("bus_side", "north")).unwrap();
        let ports = s.ports();
        assert_eq!((ports.len(), ports[0].width.get(), ports[0].offset.x, ports[0].offset.y), (1, 8, 10, 0));
        let seen = s.handle();
        let mut r = Rig::new(Box::new(s), &[(0, Value::known(BitWidth::new(8), 0x86))]);
        assert_eq!(seen.get(), 0x86);
        r.set(0, Value::known(BitWidth::new(8), 0));
        assert_eq!(seen.get(), 0);
    }

    #[test]
    fn seven_segment_port_layout() {
        let mut s = SevenSegment::new();
        s.set_options(&OptionMap::new().with("decimal_point", true)).unwrap();
        let p: Vec<_> = s.ports().iter().map(|p| (p.offset.x, p.offset.y)).collect();
        assert_eq!(p, [(20, 0), (30, 0), (20, 60), (10, 60), (0, 60), (10, 0), (0, 0), (30, 60)]);
    }
}
