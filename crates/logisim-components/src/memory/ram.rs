//! RAM (Java `Ram`). Port order and positions: see [`super::mem`] (data out first, so the
//! primary output is port 0 at (0,0)); use [`Ram::layout_ports`] helpers such as
//! [`Ram::addr_port`], [`Ram::data_in_port`], [`Ram::write_enable_port`] to find the others.
//!
//! Behaviour is a direct port of `Ram.propagate`: line-enable mode (several words per access,
//! one enable per word) and byte-enable mode (one word, optional per-byte enables), synchronous
//! or asynchronous (level-triggered) writes, read-after-write or write-after-read, optional
//! asynchronous read, and a bidirectional or separate data bus.

use logisim_core::{BitWidth, Component, Port, PropagateCtx, Value};

use super::contents::MemContents;
use super::mem::{DataBus, EnableMode, Layout, LayoutSpec, LineSize, ReadBehavior};
use super::{long_value, ClockState, Trigger, MEM_DELAY};

/// Whether contents survive a simulation reset (Java `ATTR_TYPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RamType {
    /// Cleared by [`Ram::reset_contents`].
    #[default]
    Volatile,
    NonVolatile,
}

#[derive(Debug, Clone)]
pub struct RamOptions {
    /// 2..=24
    pub addr_bits: u8,
    /// 1..=64
    pub data_bits: u8,
    pub enables: EnableMode,
    pub ram_type: RamType,
    /// Adds an asynchronous clear input.
    pub clear_pin: bool,
    pub trigger: Trigger,
    /// Read without waiting for a clock edge (edge-triggered byte mode only).
    pub async_read: bool,
    pub read_behavior: ReadBehavior,
    /// Per-byte write enables (byte mode, synchronous, data wider than 8 bits).
    pub byte_enables: bool,
    pub bus: DataBus,
    /// Words per access (line-enable mode).
    pub line: LineSize,
    pub allow_misaligned: bool,
}

impl Default for RamOptions {
    fn default() -> Self {
        RamOptions {
            addr_bits: 8,
            data_bits: 8,
            enables: EnableMode::Byte,
            ram_type: RamType::Volatile,
            clear_pin: false,
            trigger: Trigger::Rising,
            async_read: false,
            read_behavior: ReadBehavior::ReadAfterWrite,
            byte_enables: false,
            bus: DataBus::Separate,
            line: LineSize::Single,
            allow_misaligned: false,
        }
    }
}

impl RamOptions {
    pub fn addr_bits(mut self, n: u8) -> Self {
        assert!((2..=24).contains(&n), "RAM address width must be 2..=24");
        self.addr_bits = n;
        self
    }
    pub fn data_bits(mut self, n: u8) -> Self {
        assert!((1..=64).contains(&n), "RAM data width must be 1..=64");
        self.data_bits = n;
        self
    }
    pub fn enables(mut self, e: EnableMode) -> Self {
        self.enables = e;
        self
    }
    pub fn ram_type(mut self, t: RamType) -> Self {
        self.ram_type = t;
        self
    }
    pub fn clear_pin(mut self, on: bool) -> Self {
        self.clear_pin = on;
        self
    }
    pub fn trigger(mut self, t: Trigger) -> Self {
        self.trigger = t;
        self
    }
    pub fn async_read(mut self, on: bool) -> Self {
        self.async_read = on;
        self
    }
    pub fn read_behavior(mut self, r: ReadBehavior) -> Self {
        self.read_behavior = r;
        self
    }
    pub fn byte_enables(mut self, on: bool) -> Self {
        self.byte_enables = on;
        self
    }
    pub fn bus(mut self, b: DataBus) -> Self {
        self.bus = b;
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

    pub(crate) fn spec(&self) -> LayoutSpec {
        LayoutSpec {
            is_ram: true,
            addr_bits: self.addr_bits,
            data_bits: self.data_bits,
            enables: self.enables,
            line: self.line,
            bus: self.bus,
            trigger: self.trigger,
            byte_enables: self.byte_enables,
            clear_pin: self.clear_pin,
        }
    }
}

pub struct Ram {
    opts: RamOptions,
    layout: Layout,
    contents: MemContents,
    clock: ClockState,
    /// Last good address seen (Java `MemState.getCurrent`).
    current: Option<u64>,
}

impl Ram {
    pub fn new(opts: RamOptions) -> Self {
        let layout = opts.spec().layout();
        let contents = MemContents::new(opts.addr_bits, opts.data_bits);
        Ram {
            opts,
            layout,
            contents,
            clock: ClockState::default(),
            current: None,
        }
    }

    pub fn contents(&self) -> &MemContents {
        &self.contents
    }
    pub fn contents_mut(&mut self) -> &mut MemContents {
        &mut self.contents
    }
    /// Simulation reset: clears the contents of a volatile RAM (Java `Ram.reset`).
    pub fn reset_contents(&mut self) {
        if self.opts.ram_type == RamType::Volatile {
            self.contents.clear();
        }
    }

    /// Port index of data out `i` (0 is the primary output).
    pub fn data_out_port(&self, i: usize) -> usize {
        self.layout.data_out(i)
    }
    pub fn addr_port(&self) -> usize {
        self.layout.addr
    }
    /// Data-in port `i` (for a bidirectional bus this is the same port as data out `i`).
    pub fn data_in_port(&self, i: usize) -> usize {
        self.layout.data_in(i)
    }
    pub fn output_enable_port(&self) -> Option<usize> {
        self.layout.oe
    }
    pub fn write_enable_port(&self) -> Option<usize> {
        self.layout.we
    }
    pub fn clock_port(&self) -> Option<usize> {
        self.layout.clk
    }
    pub fn line_enable_port(&self, i: usize) -> Option<usize> {
        self.layout.le.get(i).copied()
    }
    /// Byte-enable port for byte `i` (byte 0 = least significant).
    pub fn byte_enable_port(&self, i: usize) -> Option<usize> {
        self.layout.be.get(i).copied()
    }
    pub fn clear_port(&self) -> Option<usize> {
        self.layout.clr
    }

    fn separate(&self) -> bool {
        self.opts.bus.is_separate()
    }
    fn data_width(&self) -> BitWidth {
        BitWidth::new(self.opts.data_bits)
    }

    fn drive_all_out(&self, ctx: &mut dyn PropagateCtx, v: Value) {
        for i in 0..self.layout.n_out {
            ctx.set_port(self.layout.data_out(i), v, MEM_DELAY);
        }
    }

    fn propagate_line_enables(
        &mut self,
        ctx: &mut dyn PropagateCtx,
        addr: u64,
        good: bool,
        error: bool,
    ) {
        let lines = self.layout.n_out.max(1);
        let misaligned = addr % lines as u64 != 0;
        let misalign_error = misaligned && !self.opts.allow_misaligned;
        let clk = self.layout.clk.map_or(Value::UNKNOWN, |p| ctx.port_value(p));
        let triggered = self.clock.update(clk, self.opts.trigger);
        let we = self
            .layout
            .we
            .is_some_and(|p| ctx.port_value(p) == Value::TRUE);
        if triggered && we && good && !misalign_error {
            for i in 0..lines {
                if lines > 1 && ctx.port_value(self.layout.le[i]) != Value::TRUE {
                    continue;
                }
                let data = long_value(ctx.port_value(self.layout.data_in(i)));
                self.contents.set(addr + i as u64, data);
            }
        }
        let width = self.data_width();
        let output_enabled = self.separate()
            || self
                .layout
                .oe
                .is_none_or(|p| ctx.port_value(p) != Value::FALSE);
        if output_enabled && good && !misalign_error {
            for i in 0..lines {
                let v = Value::known(width, self.contents.get(addr + i as u64));
                ctx.set_port(self.layout.data_out(i), v, MEM_DELAY);
            }
        } else if output_enabled && (error || (good && misalign_error)) {
            self.drive_all_out(ctx, Value::error(width));
        } else {
            self.drive_all_out(ctx, Value::unknown(width));
        }
    }

    fn propagate_byte_enables(
        &mut self,
        ctx: &mut dyn PropagateCtx,
        addr: u64,
        good: bool,
        error: bool,
    ) {
        let old = self.current.map_or(0, |a| self.contents.get(a));
        let mut new = old;
        let trigger = self.opts.trigger;
        let we_value = self
            .layout
            .we
            .map_or(Value::UNKNOWN, |p| ctx.port_value(p));
        let is_async = !trigger.is_edge();
        let edge = !is_async
            && self.clock.update(
                self.layout.clk.map_or(Value::UNKNOWN, |p| ctx.port_value(p)),
                trigger,
            );
        let we_async = (trigger == Trigger::High && we_value == Value::TRUE)
            || (trigger == Trigger::Low && we_value == Value::FALSE);
        let we_triggered = (is_async && we_async) || (edge && we_value == Value::TRUE);
        if good && we_triggered {
            let data_in = long_value(ctx.port_value(self.layout.data_in(0)));
            if self.layout.be.is_empty() {
                new = data_in;
            } else {
                for (i, &p) in self.layout.be.iter().enumerate() {
                    let mask = 0xFFu64 << (i * 8);
                    if ctx.port_value(p) == Value::TRUE {
                        new = (new & !mask) | (data_in & mask);
                    }
                }
            }
            self.contents.set(addr, new);
        }
        let bits = self.data_width();
        let out = self.layout.data_out(0);
        let oe_low = self
            .layout
            .oe
            .is_some_and(|p| ctx.port_value(p) == Value::FALSE);
        if oe_low {
            if !self.separate() || self.opts.bus == DataBus::SeparateControlled {
                ctx.set_port(out, Value::unknown(bits), MEM_DELAY);
            }
            return;
        }
        if error {
            ctx.set_port(out, Value::error(bits), MEM_DELAY);
            return;
        }
        if !good {
            ctx.set_port(out, Value::unknown(bits), MEM_DELAY);
            return;
        }
        if is_async || self.opts.async_read {
            ctx.set_port(out, Value::known(bits, new), MEM_DELAY);
            return;
        }
        if edge {
            let v = match self.opts.read_behavior {
                ReadBehavior::ReadAfterWrite => new,
                ReadBehavior::WriteAfterRead => old,
            };
            ctx.set_port(out, Value::known(bits, v), MEM_DELAY);
        }
    }
}

impl Component for Ram {
    fn name(&self) -> &str {
        "RAM"
    }
    fn ports(&self) -> Vec<Port> {
        self.layout.ports.clone()
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        if let Some(clr) = self.layout.clr {
            if ctx.port_value(clr) == Value::TRUE {
                self.contents.clear();
                let v = if self.separate() {
                    Value::known(self.data_width(), 0)
                } else {
                    Value::unknown(self.data_width())
                };
                self.drive_all_out(ctx, v);
                return;
            }
        }
        let addr_value = ctx.port_value(self.layout.addr);
        let addr = long_value(addr_value);
        let good = addr_value.is_fully_defined();
        if good && Some(addr) != self.current {
            self.current = Some(addr);
        }
        if self.opts.enables == EnableMode::Line {
            self.propagate_line_enables(ctx, addr, good, addr_value.is_error());
        } else {
            self.propagate_byte_enables(ctx, addr, good, addr_value.is_error());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::testutil::Bench;

    const T: Value = Value::TRUE;
    const F: Value = Value::FALSE;

    fn val(w: u8, v: u64) -> Value {
        Value::known(BitWidth::new(w), v)
    }

    /// Wire every input port of a RAM (except `skip`) to a pin.
    fn bench(opts: RamOptions) -> (Bench, Ram) {
        let proto = Ram::new(opts.clone());
        let (a, d) = (opts.addr_bits, opts.data_bits);
        let mut b = Bench::new(Box::new(Ram::new(opts.clone())));
        b.drive(proto.addr_port(), val(a, 0));
        if opts.bus.is_separate() {
            for i in 0..proto.layout.n_in {
                b.drive(proto.data_in_port(i), val(d, 0));
            }
        }
        for p in [proto.output_enable_port(), proto.write_enable_port()]
            .into_iter()
            .flatten()
        {
            b.drive(p, if Some(p) == proto.output_enable_port() { T } else { F });
        }
        if let Some(p) = proto.clock_port() {
            b.drive(p, F);
        }
        for i in 0..proto.layout.le.len() {
            b.drive(proto.line_enable_port(i).unwrap(), T);
        }
        for i in 0..proto.layout.be.len() {
            b.drive(proto.byte_enable_port(i).unwrap(), T);
        }
        if let Some(p) = proto.clear_port() {
            b.drive(p, F);
        }
        b.start();
        (b, proto)
    }

    fn write(b: &mut Bench, p: &Ram, addr: u64, data: u64) {
        let (a, d) = (p.opts.addr_bits, p.opts.data_bits);
        b.set(p.addr_port(), val(a, addr));
        b.set(p.data_in_port(0), val(d, data));
        b.set(p.write_enable_port().unwrap(), T);
        b.pulse(p.clock_port().unwrap());
        b.set(p.write_enable_port().unwrap(), F);
    }

    #[test]
    fn write_then_read_back() {
        let (mut b, p) = bench(RamOptions::default().addr_bits(4));
        write(&mut b, &p, 3, 0x5A);
        write(&mut b, &p, 9, 0xC3);
        b.set(p.addr_port(), val(4, 3));
        b.pulse(p.clock_port().unwrap()); // synchronous read needs a clock edge
        assert_eq!(b.out(0), val(8, 0x5A));
        b.set(p.addr_port(), val(4, 9));
        b.pulse(p.clock_port().unwrap());
        assert_eq!(b.out(0), val(8, 0xC3));
    }

    #[test]
    fn read_after_write_vs_write_after_read() {
        let (mut b, p) = bench(RamOptions::default().addr_bits(4));
        write(&mut b, &p, 1, 7);
        assert_eq!(b.out(0), val(8, 7), "default returns the new value");
        write(&mut b, &p, 1, 9);
        assert_eq!(b.out(0), val(8, 9));

        let (mut b, p) = bench(
            RamOptions::default()
                .addr_bits(4)
                .read_behavior(ReadBehavior::WriteAfterRead),
        );
        write(&mut b, &p, 1, 7);
        assert_eq!(b.out(0), val(8, 0), "old contents");
        write(&mut b, &p, 1, 9);
        assert_eq!(b.out(0), val(8, 7));
        assert_eq!(p.contents().get(1), 0, "prototype is separate from the bench part");
    }

    #[test]
    fn no_write_without_enable() {
        let (mut b, p) = bench(RamOptions::default().addr_bits(4));
        b.set(p.data_in_port(0), val(8, 1));
        b.pulse(p.clock_port().unwrap());
        b.pulse(p.clock_port().unwrap());
        assert_eq!(b.out(0), val(8, 0));
    }

    #[test]
    fn async_read_follows_address_immediately() {
        let (mut b, p) = bench(RamOptions::default().addr_bits(4).async_read(true));
        write(&mut b, &p, 5, 0x77);
        b.set(p.addr_port(), val(4, 6));
        assert_eq!(b.out(0), val(8, 0));
        b.set(p.addr_port(), val(4, 5));
        assert_eq!(b.out(0), val(8, 0x77));
    }

    #[test]
    fn output_enable_low_floats_controlled_output() {
        let (mut b, p) = bench(
            RamOptions::default()
                .addr_bits(4)
                .async_read(true)
                .bus(DataBus::SeparateControlled),
        );
        write(&mut b, &p, 2, 0x12);
        b.set(p.addr_port(), val(4, 2));
        assert_eq!(b.out(0), val(8, 0x12));
        b.set(p.output_enable_port().unwrap(), F);
        assert_eq!(b.out(0), Value::unknown(BitWidth::new(8)));
    }

    #[test]
    fn unknown_and_error_addresses() {
        let (mut b, p) = bench(RamOptions::default().addr_bits(4).async_read(true));
        b.set(p.addr_port(), Value::unknown(BitWidth::new(4)));
        assert_eq!(b.out(0), Value::unknown(BitWidth::new(8)));
        b.set(p.addr_port(), Value::error(BitWidth::new(4)));
        assert_eq!(b.out(0), Value::error(BitWidth::new(8)));
    }

    #[test]
    fn level_triggered_write_and_read() {
        let (mut b, p) = bench(RamOptions::default().addr_bits(4).trigger(Trigger::High));
        assert_eq!(p.clock_port(), None, "no clock port in async mode");
        let we = p.write_enable_port().unwrap();
        b.set(p.addr_port(), val(4, 2));
        b.set(p.data_in_port(0), val(8, 0x33));
        b.set(we, T);
        assert_eq!(b.out(0), val(8, 0x33), "asynchronous read shows the write");
        b.set(we, F);
        b.set(p.data_in_port(0), val(8, 0x44));
        assert_eq!(b.out(0), val(8, 0x33));
        let _ = p;
    }

    #[test]
    fn byte_enables_merge_bytes() {
        let (mut b, p) = bench(
            RamOptions::default()
                .addr_bits(4)
                .data_bits(16)
                .byte_enables(true),
        );
        write(&mut b, &p, 0, 0xAABB);
        assert_eq!(b.out(0), val(16, 0xAABB));
        // enable only the high byte
        b.set(p.byte_enable_port(0).unwrap(), F);
        write(&mut b, &p, 0, 0x1122);
        assert_eq!(b.out(0), val(16, 0x11BB));
        // enable only the low byte
        b.set(p.byte_enable_port(0).unwrap(), T);
        b.set(p.byte_enable_port(1).unwrap(), F);
        write(&mut b, &p, 0, 0x3344);
        assert_eq!(b.out(0), val(16, 0x1144));
    }

    #[test]
    fn clear_pin_wipes_memory() {
        let (mut b, p) = bench(RamOptions::default().addr_bits(4).clear_pin(true));
        write(&mut b, &p, 4, 0x99);
        b.set(p.clear_port().unwrap(), T);
        assert_eq!(b.out(0), val(8, 0));
        b.set(p.clear_port().unwrap(), F);
        b.set(p.addr_port(), val(4, 4));
        b.pulse(p.clock_port().unwrap());
        assert_eq!(b.out(0), val(8, 0));
    }

    #[test]
    fn bidirectional_bus_shares_one_data_port() {
        let (mut b, p) = bench(
            RamOptions::default()
                .addr_bits(4)
                .bus(DataBus::Bidir),
        );
        assert_eq!(p.data_in_port(0), 0);
        // drive the shared data port as an external writer
        b.drive(0, val(8, 0xEE));
        b.set(p.output_enable_port().unwrap(), F);
        b.set(p.addr_port(), val(4, 1));
        b.set(p.write_enable_port().unwrap(), T);
        b.pulse(p.clock_port().unwrap());
        b.set(p.write_enable_port().unwrap(), F);
        // stop driving and let the RAM answer
        b.set(0, Value::unknown(BitWidth::new(8)));
        b.set(p.output_enable_port().unwrap(), T);
        b.pulse(p.clock_port().unwrap());
        assert_eq!(b.out(0), val(8, 0xEE));
    }

    #[test]
    fn line_enables_write_whole_lines() {
        let opts = RamOptions::default()
            .addr_bits(4)
            .enables(EnableMode::Line)
            .line(LineSize::Dual);
        let (mut b, p) = bench(opts);
        for (i, v) in [0x11u64, 0x22].into_iter().enumerate() {
            b.set(p.data_in_port(i), val(8, v));
        }
        b.set(p.addr_port(), val(4, 2));
        b.set(p.write_enable_port().unwrap(), T);
        b.pulse(p.clock_port().unwrap());
        assert_eq!(b.out(p.data_out_port(0)), val(8, 0x11));
        assert_eq!(b.out(p.data_out_port(1)), val(8, 0x22));
        // only the second word enabled
        b.set(p.line_enable_port(0).unwrap(), F);
        b.set(p.data_in_port(0), val(8, 0xAA));
        b.set(p.data_in_port(1), val(8, 0xBB));
        b.pulse(p.clock_port().unwrap());
        assert_eq!(b.out(p.data_out_port(0)), val(8, 0x11));
        assert_eq!(b.out(p.data_out_port(1)), val(8, 0xBB));
    }

    #[test]
    fn misaligned_line_access_is_an_error_unless_allowed() {
        let mk = |allow| {
            let opts = RamOptions::default()
                .addr_bits(4)
                .enables(EnableMode::Line)
                .line(LineSize::Dual)
                .allow_misaligned(allow);
            bench(opts)
        };
        let (mut b, p) = mk(false);
        b.set(p.addr_port(), val(4, 1));
        b.pulse(p.clock_port().unwrap());
        assert_eq!(b.out(0), Value::error(BitWidth::new(8)));
        let (mut b, p) = mk(true);
        b.set(p.addr_port(), val(4, 1));
        b.pulse(p.clock_port().unwrap());
        assert_eq!(b.out(0), val(8, 0));
    }

    #[test]
    fn volatile_reset_clears_nonvolatile_keeps() {
        let mut r = Ram::new(RamOptions::default());
        r.contents_mut().set(1, 5);
        r.reset_contents();
        assert_eq!(r.contents().get(1), 0);
        let mut r = Ram::new(RamOptions::default().ram_type(RamType::NonVolatile));
        r.contents_mut().set(1, 5);
        r.reset_contents();
        assert_eq!(r.contents().get(1), 5);
    }
}
