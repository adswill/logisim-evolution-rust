//! Types and port layout shared by RAM and ROM (Java `Mem` + `RamAppearance`).
//!
//! Ports of a memory (east-facing, classic layout) in index order: data out `0..n_out`
//! (primary output at (0,0), consecutive outputs 10 units lower), address, data in
//! (separate-bus RAM only), output enable, write enable, clock, line enables, byte enables,
//! clear. A port that does not exist for the chosen options has no index (`None`).
//! Java places the origin at the top-left; every position is translated so data out 0 is at (0,0).

use logisim_core::{BitWidth, Port, PortKind};

use super::Trigger;

/// How wide a memory access is, in words (Java `LINE_ATTR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineSize {
    #[default]
    Single,
    Dual,
    Quad,
    Octo,
}

impl LineSize {
    pub fn words(self) -> usize {
        match self {
            LineSize::Single => 1,
            LineSize::Dual => 2,
            LineSize::Quad => 4,
            LineSize::Octo => 8,
        }
    }
}

/// Which write-enable scheme a RAM uses (Java `ENABLES_ATTR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnableMode {
    /// One word per access; optional per-byte enables for words wider than 8 bits.
    #[default]
    Byte,
    /// Several words per access (see [`LineSize`]), each with its own line enable.
    Line,
}

/// RAM data bus style (Java `ATTR_DBUS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataBus {
    /// One shared bidirectional data port.
    Bidir,
    /// Separate data-in and data-out ports (Logisim default).
    #[default]
    Separate,
    /// Separate ports, but the output floats unless output-enable is high.
    SeparateControlled,
}

impl DataBus {
    pub fn is_separate(self) -> bool {
        !matches!(self, DataBus::Bidir)
    }
}

/// What a synchronous read returns on a write cycle (Java `READ_ATTR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadBehavior {
    /// The newly written value.
    #[default]
    ReadAfterWrite,
    /// The previous contents.
    WriteAfterRead,
}

pub(crate) const LINES: [(&str, LineSize); 4] = [
    ("single", LineSize::Single),
    ("dual", LineSize::Dual),
    ("quad", LineSize::Quad),
    ("octo", LineSize::Octo),
];
pub(crate) const ENABLES: [(&str, EnableMode); 2] =
    [("byte", EnableMode::Byte), ("line", EnableMode::Line)];
pub(crate) const BUSES: [(&str, DataBus); 3] = [
    ("separate", DataBus::Separate),
    ("separate_controlled", DataBus::SeparateControlled),
    ("bidir", DataBus::Bidir),
];
pub(crate) const READS: [(&str, ReadBehavior); 2] = [
    ("read_after_write", ReadBehavior::ReadAfterWrite),
    ("write_after_read", ReadBehavior::WriteAfterRead),
];

/// Everything the layout depends on.
#[derive(Debug, Clone)]
pub(crate) struct LayoutSpec {
    /// `false` for ROM (which has no enables, trigger or bus attributes).
    pub is_ram: bool,
    pub addr_bits: u8,
    pub data_bits: u8,
    pub enables: EnableMode,
    pub line: LineSize,
    pub bus: DataBus,
    pub trigger: Trigger,
    pub byte_enables: bool,
    pub clear_pin: bool,
}

/// Computed port indices and positions.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pub n_out: usize,
    pub n_in: usize,
    pub addr: usize,
    pub oe: Option<usize>,
    pub we: Option<usize>,
    pub clk: Option<usize>,
    pub le: Vec<usize>,
    pub be: Vec<usize>,
    pub clr: Option<usize>,
    pub ports: Vec<Port>,
}

impl Layout {
    pub fn data_out(&self, i: usize) -> usize {
        debug_assert!(i < self.n_out);
        i
    }
    pub fn data_in(&self, i: usize) -> usize {
        if self.n_in == 0 {
            i // bidirectional: shared with data out
        } else {
            self.addr + 1 + i
        }
    }
}

impl LayoutSpec {
    fn separate(&self) -> bool {
        self.is_ram && self.bus.is_separate()
    }
    fn synchronous(&self) -> bool {
        self.is_ram && self.trigger.is_edge()
    }
    fn line_mode(&self) -> bool {
        self.is_ram && self.enables == EnableMode::Line
    }
    fn n_data_out(&self) -> usize {
        if !self.is_ram || self.line_mode() {
            self.line.words()
        } else {
            1
        }
    }
    fn n_data_in(&self) -> usize {
        if self.separate() {
            self.n_data_out()
        } else {
            0
        }
    }
    fn n_oe(&self) -> usize {
        usize::from(self.is_ram && (!self.separate() || self.enables == EnableMode::Byte))
    }
    fn n_we(&self) -> usize {
        usize::from(self.is_ram)
    }
    fn n_clk(&self) -> usize {
        usize::from(self.is_ram && (self.synchronous() || self.line_mode()))
    }
    fn n_le(&self) -> usize {
        if self.line_mode() {
            match self.line {
                LineSize::Single => 0,
                l => l.words(),
            }
        } else {
            0
        }
    }
    fn n_be(&self) -> usize {
        if self.is_ram
            && self.enables == EnableMode::Byte
            && self.byte_enables
            && self.synchronous()
            && self.data_bits >= 9
        {
            (usize::from(self.data_bits) + 7) >> 3
        } else {
            0
        }
    }
    fn n_clr(&self) -> usize {
        usize::from(self.is_ram && self.clear_pin)
    }

    fn control_height(&self) -> i32 {
        let mut h = 60;
        if self.line_mode() {
            h += self.n_le() as i32 * 10;
        } else if self.is_ram {
            h += 20;
            if self.synchronous() {
                h += 10;
            }
            h += self.n_be() as i32 * 10;
        }
        h
    }

    pub fn layout(&self) -> Layout {
        let (n_out, n_in) = (self.n_data_out(), self.n_data_in());
        let (n_oe, n_we, n_clk, n_le, n_be, n_clr) = (
            self.n_oe(),
            self.n_we(),
            self.n_clk(),
            self.n_le(),
            self.n_be(),
            self.n_clr(),
        );
        let ch = self.control_height();
        let line = self.line_mode();
        let data = BitWidth::new(self.data_bits);
        let one = BitWidth::ONE;
        let out_kind = if !self.separate() && self.is_ram {
            PortKind::InOut
        } else {
            PortKind::Output
        };
        // Java coordinates (origin top-left); translated below
        let width = 240;
        let mut ports: Vec<(i32, i32, PortKind, BitWidth)> = vec![];
        for i in 0..n_out {
            ports.push((width, ch + 10 * i as i32, out_kind, data));
        }
        let addr = ports.len();
        ports.push((0, 10, PortKind::Input, BitWidth::new(self.addr_bits)));
        for i in 0..n_in {
            ports.push((0, ch + 10 * i as i32, PortKind::Input, data));
        }
        let mut next = |count: usize, mk: &mut dyn FnMut(usize) -> (i32, i32)| -> Vec<usize> {
            (0..count)
                .map(|i| {
                    let (x, y) = mk(i);
                    ports.push((x, y, PortKind::Input, one));
                    ports.len() - 1
                })
                .collect()
        };
        let oe = next(n_oe, &mut |_| (0, if line { 20 } else { 60 })).first().copied();
        let we = next(n_we, &mut |_| (0, if line { 30 } else { 50 })).first().copied();
        let clk_y = (if line { 40 } else { 70 }) + n_le as i32 * 10 + n_be as i32 * 10;
        let clk = next(n_clk, &mut |_| (0, clk_y)).first().copied();
        let le = next(n_le, &mut |i| (0, if line { 40 } else { 70 } + i as i32 * 10));
        let be = next(n_be, &mut |i| (0, 70 + (n_be - i - 1) as i32 * 10));
        let clr = next(n_clr, &mut |_| (40, 0)).first().copied();

        let (ox, oy) = (ports[0].0, ports[0].1);
        Layout {
            n_out,
            n_in,
            addr,
            oe,
            we,
            clk,
            le,
            be,
            clr,
            ports: ports
                .into_iter()
                .map(|(x, y, k, w)| Port::new(x - ox, y - oy, k, w))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ram() -> LayoutSpec {
        LayoutSpec {
            is_ram: true,
            addr_bits: 8,
            data_bits: 8,
            enables: EnableMode::Byte,
            line: LineSize::Single,
            bus: DataBus::Separate,
            trigger: Trigger::Rising,
            byte_enables: false,
            clear_pin: false,
        }
    }

    #[test]
    fn default_ram_layout() {
        let l = ram().layout();
        // data out, addr, data in, oe, we, clk
        assert_eq!(l.ports.len(), 6);
        assert_eq!((l.ports[0].offset.x, l.ports[0].offset.y), (0, 0));
        assert_eq!(l.addr, 1);
        assert_eq!((l.oe, l.we, l.clk), (Some(3), Some(4), Some(5)));
        assert!(l.ports.iter().all(|p| p.offset.x % 10 == 0 && p.offset.y % 10 == 0));
    }

    #[test]
    fn bidir_has_no_separate_input_and_inout_port() {
        let mut s = ram();
        s.bus = DataBus::Bidir;
        let l = s.layout();
        assert_eq!(l.n_in, 0);
        assert_eq!(l.ports[0].kind, PortKind::InOut);
        assert_eq!(l.data_in(0), 0);
    }

    #[test]
    fn line_enables_quad() {
        let mut s = ram();
        s.enables = EnableMode::Line;
        s.line = LineSize::Quad;
        let l = s.layout();
        assert_eq!(l.n_out, 4);
        assert_eq!(l.n_in, 4);
        assert_eq!(l.le.len(), 4);
        assert_eq!(l.oe, None, "separate bus + line enables has no OE");
    }

    #[test]
    fn byte_enables_only_for_wide_sync_words() {
        let mut s = ram();
        s.data_bits = 24;
        s.byte_enables = true;
        assert_eq!(s.layout().be.len(), 3);
        s.trigger = Trigger::High;
        assert_eq!(s.layout().be.len(), 0);
        assert_eq!(s.layout().clk, None);
        s.trigger = Trigger::Rising;
        s.data_bits = 8;
        assert_eq!(s.layout().be.len(), 0);
    }

    #[test]
    fn rom_layout_is_addr_plus_outputs() {
        let s = LayoutSpec {
            is_ram: false,
            line: LineSize::Dual,
            ..ram()
        };
        let l = s.layout();
        assert_eq!(l.ports.len(), 3);
        assert_eq!(l.n_out, 2);
        assert_eq!(l.ports[0].kind, PortKind::Output);
        assert_eq!((l.ports[1].offset.x, l.ports[1].offset.y), (0, 10));
        assert_eq!(l.ports[0].offset.y, 0);
        // addr is 240 left and (60 - 10) above data out 0
        assert_eq!((l.ports[2].offset.x, l.ports[2].offset.y), (-240, 10 - 60));
    }
}
