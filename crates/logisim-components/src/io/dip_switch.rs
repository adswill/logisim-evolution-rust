//! DIP switch: `n` independent toggle switches, each driving a 1-bit output.
//!
//! Port order: `i` = switch i, at (0, 10*(i+1)) (east-facing layout). The UI toggles bits through
//! [`DipSwitch::handle`] and calls `Circuit::refresh(id)`.

use std::cell::Cell;
use std::rc::Rc;

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

/// Initial switch positions when the simulation resets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DipReset {
    /// All switches open (low).
    #[default]
    Open,
    /// All switches closed (high).
    Closed,
    /// Keep the saved positions.
    Last,
}

const RESETS: [(&str, DipReset); 3] = [("open", DipReset::Open), ("closed", DipReset::Closed), ("last", DipReset::Last)];

#[derive(Debug, Clone)]
pub struct DipSwitchOptions {
    /// Number of switches, 1..=32.
    pub switches: u8,
    pub reset: DipReset,
    /// Saved positions (bit i = switch i closed); used with [`DipReset::Last`].
    pub positions: u32,
}

impl Default for DipSwitchOptions {
    fn default() -> Self {
        DipSwitchOptions { switches: 8, reset: DipReset::Open, positions: 0 }
    }
}

impl DipSwitchOptions {
    pub fn switches(mut self, n: u8) -> Self {
        assert!((1..=32).contains(&n), "DIP switches must be 1..=32");
        self.switches = n;
        self
    }
    pub fn reset(mut self, r: DipReset) -> Self {
        self.reset = r;
        self
    }
    pub fn positions(mut self, p: u32) -> Self {
        self.positions = p;
        self
    }
}

fn mask(n: u8) -> u32 {
    if n >= 32 {
        u32::MAX
    } else {
        (1u32 << n) - 1
    }
}

fn initial_value(o: &DipSwitchOptions) -> u32 {
    match o.reset {
        DipReset::Open => 0,
        DipReset::Closed => mask(o.switches),
        DipReset::Last => o.positions & mask(o.switches),
    }
}

/// Current switch positions (bit i set = closed/high).
pub type DipHandle = Rc<Cell<u32>>;

pub struct DipSwitch {
    opts: DipSwitchOptions,
    state: DipHandle,
}

impl DipSwitch {
    pub fn new(opts: DipSwitchOptions) -> Self {
        let state = Rc::new(Cell::new(initial_value(&opts)));
        DipSwitch { opts, state }
    }
    pub fn handle(&self) -> DipHandle {
        Rc::clone(&self.state)
    }
    /// Flip switch `i` (ignored if out of range).
    pub fn toggle(handle: &DipHandle, i: u8, switches: u8) {
        if i < switches {
            handle.set(handle.get() ^ (1 << i));
        }
    }
}

impl Component for DipSwitch {
    fn name(&self) -> &str {
        "DIP Switch"
    }
    fn options(&self) -> OptionMap {
        let o = &self.opts;
        OptionMap::new()
            .with("switches", o.switches)
            .with("reset", OptionMap::name_of(o.reset, &RESETS))
            // Persist the live positions so "last" restores what the user left.
            .with("positions", (self.state.get() & mask(o.switches)) as i64)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("switches", "Number of switches", 1, 32, 8),
            OptionSpec::choice("reset", "Switches at reset", &["open", "closed", "last"], "open"),
            OptionSpec::int("positions", "Saved positions", 0, u32::MAX as i64, 0),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["switches", "reset", "positions"])?;
        let o = &mut self.opts;
        if let Some(v) = m.int("switches", 1, 32)? {
            o.switches = v as u8;
        }
        o.reset = m.choice("reset", &RESETS)?.unwrap_or(o.reset);
        if let Some(v) = m.int("positions", 0, u32::MAX as i64)? {
            o.positions = v as u32;
        }
        self.state.set(initial_value(&self.opts));
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        (0..self.opts.switches as i32).map(|i| Port::new(0, 10 * (i + 1), PortKind::Output, BitWidth::ONE)).collect()
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let bits = self.state.get();
        for i in 0..self.opts.switches as usize {
            ctx.set_port(i, if bits >> i & 1 == 1 { Value::TRUE } else { Value::FALSE }, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::testutil::Rig;

    #[test]
    fn toggling_drives_outputs() {
        let d = DipSwitch::new(DipSwitchOptions::default().switches(4));
        let h = d.handle();
        let mut r = Rig::new(Box::new(d), &[]);
        assert!((0..4).all(|i| r.get(i) == Value::FALSE));
        DipSwitch::toggle(&h, 2, 4);
        DipSwitch::toggle(&h, 9, 4); // out of range: ignored
        r.refresh();
        let got: Vec<_> = (0..4).map(|i| r.get(i)).collect();
        assert_eq!(got, [Value::FALSE, Value::FALSE, Value::TRUE, Value::FALSE]);
    }

    #[test]
    fn reset_modes() {
        let closed = DipSwitch::new(DipSwitchOptions::default().switches(3).reset(DipReset::Closed));
        assert_eq!(closed.handle().get(), 0b111);
        let last = DipSwitch::new(DipSwitchOptions::default().switches(3).reset(DipReset::Last).positions(0b1101));
        assert_eq!(last.handle().get(), 0b101);
    }

    #[test]
    fn options_save_live_positions() {
        let d = DipSwitch::new(DipSwitchOptions::default().reset(DipReset::Last));
        d.handle().set(0b1010);
        let saved = d.options();
        let mut fresh = DipSwitch::new(DipSwitchOptions::default());
        fresh.set_options(&saved).unwrap();
        assert_eq!(fresh.handle().get(), 0b1010);
    }
}
