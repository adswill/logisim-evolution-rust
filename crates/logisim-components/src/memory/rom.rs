//! ROM (Java `Rom`). Ports: data out `0..n` (port 0 at (0,0), one per word of the line size),
//! then address. Reads are combinational. Use [`RomOptions::contents`] or
//! [`RomOptions::with_data`] to load the program.

use logisim_core::{BitWidth, Component, OptionMap, Port, PropagateCtx, Value};

use super::contents::MemContents;
use super::mem::{Layout, LayoutSpec, LineSize, LINES};
use super::{long_value, MEM_DELAY, Trigger};
use super::mem::{DataBus, EnableMode};

#[derive(Debug, Clone)]
pub struct RomOptions {
    addr_bits: u8,
    data_bits: u8,
    pub line: LineSize,
    pub allow_misaligned: bool,
    contents: MemContents,
}

impl Default for RomOptions {
    fn default() -> Self {
        RomOptions {
            addr_bits: 8,
            data_bits: 8,
            line: LineSize::Single,
            allow_misaligned: false,
            contents: MemContents::new(8, 8),
        }
    }
}

impl RomOptions {
    /// Address width, 2..=24. Existing contents are truncated as in Logisim.
    pub fn addr_bits(mut self, n: u8) -> Self {
        assert!((2..=24).contains(&n), "ROM address width must be 2..=24");
        self.addr_bits = n;
        self.contents.set_dimensions(n, self.data_bits);
        self
    }
    /// Data width, 1..=64.
    pub fn data_bits(mut self, n: u8) -> Self {
        assert!((1..=64).contains(&n), "ROM data width must be 1..=64");
        self.data_bits = n;
        self.contents.set_dimensions(self.addr_bits, n);
        self
    }
    pub fn line(mut self, l: LineSize) -> Self {
        self.line = l;
        self
    }
    pub fn allow_misaligned(mut self, on: bool) -> Self {
        self.allow_misaligned = on;
        self
    }
    /// Replaces the contents; its geometry becomes the ROM's.
    pub fn contents(mut self, c: MemContents) -> Self {
        self.addr_bits = c.addr_bits();
        self.data_bits = c.width();
        self.contents = c;
        self
    }
    /// Loads `words` at address 0 (after the width options have been set).
    pub fn with_data(mut self, words: &[u64]) -> Self {
        self.contents.set_range(0, words);
        self
    }
}

pub struct Rom {
    opts: RomOptions,
    layout: Layout,
}

impl Rom {
    pub fn new(opts: RomOptions) -> Self {
        let layout = LayoutSpec {
            is_ram: false,
            addr_bits: opts.addr_bits,
            data_bits: opts.data_bits,
            enables: EnableMode::Byte,
            line: opts.line,
            bus: DataBus::Separate,
            trigger: Trigger::Rising,
            byte_enables: false,
            clear_pin: false,
        }
        .layout();
        Rom { opts, layout }
    }
    pub fn contents(&self) -> &MemContents {
        &self.opts.contents
    }
    pub fn contents_mut(&mut self) -> &mut MemContents {
        &mut self.opts.contents
    }
    pub fn data_out_port(&self, i: usize) -> usize {
        self.layout.data_out(i)
    }
    pub fn addr_port(&self) -> usize {
        self.layout.addr
    }
}

impl Component for Rom {
    fn name(&self) -> &str {
        "ROM"
    }
    fn options(&self) -> OptionMap {
        let o = &self.opts;
        OptionMap::new()
            .with("addr_bits", o.addr_bits)
            .with("data_bits", o.data_bits)
            .with("line", OptionMap::name_of(o.line, &LINES))
            .with("allow_misaligned", o.allow_misaligned)
            .with("contents", o.contents.to_text())
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["addr_bits", "data_bits", "line", "allow_misaligned", "contents"])?;
        let mut o = self.opts.clone();
        if let Some(v) = m.int("addr_bits", 2, 24)? {
            o = o.addr_bits(v as u8);
        }
        if let Some(v) = m.int("data_bits", 1, 64)? {
            o = o.data_bits(v as u8);
        }
        o.line = m.choice("line", &LINES)?.unwrap_or(o.line);
        o.allow_misaligned = m.boolean("allow_misaligned")?.unwrap_or(o.allow_misaligned);
        if let Some(text) = m.string("contents")? {
            o.contents.clear();
            o.contents.load_text(text)?;
        }
        *self = Rom::new(o);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        self.layout.ports.clone()
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let bits = BitWidth::new(self.opts.data_bits);
        let lines = self.layout.n_out;
        let addr_value = ctx.port_value(self.layout.addr);
        let drive_all = |ctx: &mut dyn PropagateCtx, v: Value| {
            for i in 0..lines {
                ctx.set_port(self.layout.data_out(i), v, MEM_DELAY);
            }
        };
        if addr_value.is_error() {
            return drive_all(ctx, Value::error(bits));
        }
        if !addr_value.is_fully_defined() {
            return drive_all(ctx, Value::unknown(bits));
        }
        let addr = long_value(addr_value);
        let misalign_error = !addr.is_multiple_of(lines as u64) && !self.opts.allow_misaligned;
        for i in 0..lines {
            let v = if misalign_error {
                Value::error(bits)
            } else {
                Value::known(bits, self.opts.contents.get(addr + i as u64))
            };
            ctx.set_port(self.layout.data_out(i), v, MEM_DELAY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::testutil::Bench;

    fn val(w: u8, v: u64) -> Value {
        Value::known(BitWidth::new(w), v)
    }

    fn bench(opts: RomOptions) -> (Bench, Rom) {
        let a = opts.addr_bits;
        let proto = Rom::new(opts.clone());
        let mut b = Bench::new(Box::new(Rom::new(opts)));
        b.drive(proto.addr_port(), val(a, 0));
        b.start();
        (b, proto)
    }

    #[test]
    fn reads_stored_words() {
        let opts = RomOptions::default()
            .addr_bits(4)
            .data_bits(12)
            .with_data(&[0x123, 0xABC, 0xFFF]);
        let (mut b, p) = bench(opts);
        assert_eq!(b.out(0), val(12, 0x123));
        b.set(p.addr_port(), val(4, 1));
        assert_eq!(b.out(0), val(12, 0xABC));
        b.set(p.addr_port(), val(4, 7));
        assert_eq!(b.out(0), val(12, 0), "unwritten words read 0");
    }

    #[test]
    fn undefined_addresses() {
        let (mut b, p) = bench(RomOptions::default().addr_bits(4));
        b.set(p.addr_port(), Value::unknown(BitWidth::new(4)));
        assert_eq!(b.out(0), Value::unknown(BitWidth::new(8)));
        b.set(p.addr_port(), Value::error(BitWidth::new(4)));
        assert_eq!(b.out(0), Value::error(BitWidth::new(8)));
    }

    #[test]
    fn wide_line_reads_consecutive_words() {
        let opts = RomOptions::default()
            .addr_bits(4)
            .line(LineSize::Quad)
            .with_data(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let (mut b, p) = bench(opts);
        b.set(p.addr_port(), val(4, 4));
        let got: Vec<_> = (0..4).map(|i| b.out(p.data_out_port(i))).collect();
        assert_eq!(got, [val(8, 5), val(8, 6), val(8, 7), val(8, 8)]);
        b.set(p.addr_port(), val(4, 2));
        assert_eq!(b.out(0), Value::error(BitWidth::new(8)), "misaligned");
    }

    #[test]
    fn contents_option_adopts_geometry() {
        let c = MemContents::from_values(6, 16, &[0xBEEF]);
        let opts = RomOptions::default().contents(c);
        let (mut b, _) = bench(opts);
        assert_eq!(b.out(0), val(16, 0xBEEF));
    }

    #[test]
    fn options_round_trip_with_contents() {
        let opts = RomOptions::default()
            .addr_bits(5)
            .data_bits(10)
            .line(LineSize::Dual)
            .with_data(&[0x3FF, 0x3FF, 0, 0, 0x155]);
        let rom = Rom::new(opts);
        let saved = rom.options();
        let mut fresh = Rom::new(RomOptions::default());
        fresh.set_options(&saved).unwrap();
        assert_eq!(fresh.options(), saved);
        assert_eq!(fresh.contents().get(4), 0x155);
        assert_eq!(fresh.ports(), rom.ports());
        assert!(fresh.set_options(&OptionMap::new().with("contents", "fff")).is_err());
    }

    #[test]
    fn registry_builds_every_part() {
        let lib = crate::memory::library();
        assert_eq!(lib.parts.len(), 11);
        for p in &lib.parts {
            assert_eq!((p.make)().name(), p.name);
        }
        assert!(crate::all_libraries().iter().any(|l| l.name == "Memory"));
    }
}
