//! The Wiring library: Pin, Clock, Splitter, Tunnel, Pull Resistor, Constant, Probe,
//! Power, Ground, Transistor, Transmission Gate and Bit Extender.
//!
//! Ports are listed for the east-facing orientation; `Circuit` rotates them.

use crate::registry::{Library, PartDesc};

mod extras;
pub use extras::{DoNotConnect, PorTransition, PowerOnReset};
use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};
use std::cell::Cell;
use std::rc::Rc;

pub fn library() -> Library {
    let part = |name, make: fn() -> Box<dyn Component>| PartDesc { name, make };
    Library {
        name: "Wiring",
        parts: vec![
            part("Splitter", || Box::new(Splitter::new(SplitterOptions::default()))),
            part("Pin", || Box::new(Pin::bit(false))),
            part("Probe", || Box::new(Probe::new())),
            part("Tunnel", || Box::new(Tunnel::new("", 1))),
            part("Pull Resistor", || Box::new(PullResistor::new(Value::FALSE))),
            part("Clock", || Box::new(Clock::default())),
            part("Constant", || Box::new(Constant::new(1, 1))),
            part("Power", || Box::new(Power::new(1))),
            part("Ground", || Box::new(Ground::new(1))),
            part("Transistor", || Box::new(Transistor::new(TransistorType::P, 1))),
            part("Transmission Gate", || Box::new(TransmissionGate::new(1))),
            part("Do Not Connect", || Box::new(DoNotConnect::new(1))),
            part("Power On Reset", || Box::new(PowerOnReset::new(PorTransition::HighToLow, 2))),
            part("Bit Extender", || Box::new(BitExtender::new(8, 16, ExtendKind::Zero))),
        ],
    }
}

fn bw(w: u8) -> BitWidth {
    BitWidth::new(w)
}

// ---- Pin / Clock ---------------------------------------------------------------

/// Shared handle for changing a [`Pin`]'s value after it was placed in a circuit.
/// Call `Circuit::refresh(id)` afterwards to propagate the change.
pub type PinHandle = Rc<Cell<Value>>;

/// A circuit pin. An *input* pin drives its settable value on port 0 at (0,0). An *output* pin
/// only observes the net (port 0 is an input). A non-empty `label` makes the pin part of the
/// circuit's interface when the circuit is used as a subcircuit.
pub struct Pin {
    value: PinHandle,
    output: bool,
    label: String,
    seen: PinHandle,
}

impl Pin {
    pub fn new(value: Value) -> Self {
        Pin { value: Rc::new(Cell::new(value)), output: false, label: String::new(), seen: Rc::new(Cell::new(Value::NIL)) }
    }
    /// An output pin of the given width.
    pub fn output(width: u8) -> Self {
        Pin { output: true, ..Pin::new(Value::unknown(bw(width))) }
    }
    pub fn handle(&self) -> PinHandle {
        Rc::clone(&self.value)
    }
    /// For output pins: the value last seen on the net.
    pub fn seen_handle(&self) -> PinHandle {
        Rc::clone(&self.seen)
    }
    pub fn bit(high: bool) -> Self {
        Pin::new(if high { Value::TRUE } else { Value::FALSE })
    }
    pub fn is_output(&self) -> bool {
        self.output
    }
}

impl Component for Pin {
    fn name(&self) -> &str {
        "Pin"
    }
    fn options(&self) -> OptionMap {
        let v = self.value.get();
        OptionMap::new()
            .with("width", v.width().get())
            .with("value", v.to_u64().unwrap_or(0) as i64)
            .with("output", self.output)
            .with("label", self.label.as_str())
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("width", "Data bits", 1, 64, 1),
            OptionSpec::int("value", "Value", i64::MIN, i64::MAX, 0),
            OptionSpec::boolean("output", "Output pin (shows the value)", false),
            OptionSpec::string("label", "Label (makes it a port of this circuit)", ""),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width", "value", "output", "label"])?;
        let cur = self.value.get();
        let width = m.int("width", 1, 64)?.map_or(cur.width(), |v| BitWidth::new(v as u8));
        let value = m.int("value", i64::MIN, i64::MAX)?.map_or(cur.to_u64().unwrap_or(0), |v| v as u64);
        self.output = m.boolean("output")?.unwrap_or(self.output);
        if let Some(l) = m.string("label")? {
            self.label = l.to_string();
        }
        self.value.set(Value::known(width, value));
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let kind = if self.output { PortKind::Input } else { PortKind::Output };
        vec![Port::new(0, 0, kind, self.value.get().width())]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        if self.output {
            self.seen.set(ctx.port_value(0));
        } else {
            ctx.set_port(0, self.value.get(), 1);
        }
    }
}

/// A 1-bit clock. Each tick the output is low for `low` ticks then high for `high` ticks,
/// shifted by `phase` (ported from Logisim's `Clock.ClockState`).
#[derive(Debug, Clone)]
pub struct Clock {
    pub high: u32,
    pub low: u32,
    pub phase: i64,
    ticks: u64,
    sending: Value,
}

impl Default for Clock {
    fn default() -> Self {
        Clock::new(1, 1, 0)
    }
}

impl Clock {
    pub fn new(high: u32, low: u32, phase: i64) -> Self {
        assert!(high >= 1 && low >= 1, "clock durations must be at least 1 tick");
        let mut c = Clock { high, low, phase, ticks: 0, sending: Value::UNKNOWN };
        c.update();
        c
    }
    fn update(&mut self) {
        let cycle = self.high as i64 + self.low as i64;
        let phase = self.phase.rem_euclid(cycle);
        let is_low = (self.ticks as i64 + phase) % cycle < self.low as i64;
        self.sending = if is_low { Value::FALSE } else { Value::TRUE };
    }
    /// Current output level.
    pub fn level(&self) -> Value {
        self.sending
    }
}

impl Component for Clock {
    fn name(&self) -> &str {
        "Clock"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("high", self.high).with("low", self.low).with("phase", self.phase)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("high", "High duration (ticks)", 1, u32::MAX as i64, 1),
            OptionSpec::int("low", "Low duration (ticks)", 1, u32::MAX as i64, 1),
            OptionSpec::int("phase", "Phase offset (ticks)", i64::MIN, i64::MAX, 0),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["high", "low", "phase"])?;
        let high = m.int("high", 1, u32::MAX as i64)?.map_or(self.high, |v| v as u32);
        let low = m.int("low", 1, u32::MAX as i64)?.map_or(self.low, |v| v as u32);
        let phase = m.int("phase", i64::MIN, i64::MAX)?.unwrap_or(self.phase);
        *self = Clock::new(high, low, phase);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, BitWidth::ONE)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        ctx.set_port(0, self.sending, 1);
    }
    fn tick(&mut self, ctx: &mut dyn PropagateCtx) {
        self.ticks += 1;
        self.update();
        ctx.set_port(0, self.sending, 1);
    }
}

// ---- Splitter --------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplitterAppearance {
    #[default]
    Left,
    Right,
    Center,
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitterOptions {
    /// Number of split ends (1..=64).
    pub fanout: usize,
    /// Bits on the combined side (1..=64).
    pub width: u8,
    pub appearance: SplitterAppearance,
    /// Spacing between ends in grid units (1..=?); 1 = 10 px.
    pub spacing: i32,
    /// For each combined bit, which end (1..=fanout) it goes to; 0 = nowhere.
    pub bit_end: Vec<u8>,
}

impl Default for SplitterOptions {
    fn default() -> Self {
        SplitterOptions::new(2, 2)
    }
}

impl SplitterOptions {
    pub fn new(fanout: usize, width: u8) -> Self {
        assert!((1..=64).contains(&fanout) && (1..=64).contains(&width));
        SplitterOptions {
            fanout,
            width,
            appearance: SplitterAppearance::Left,
            spacing: 1,
            bit_end: compute_distribution(fanout, width as usize, true),
        }
    }
    /// Reassign every bit to ends in ascending (or descending) order.
    pub fn distribute(mut self, ascending: bool) -> Self {
        self.bit_end = compute_distribution(self.fanout, self.width as usize, ascending);
        self
    }
    pub fn map_bit(mut self, bit: usize, end: u8) -> Self {
        assert!(end as usize <= self.fanout);
        self.bit_end[bit] = end;
        self
    }
}

/// Default bit→end distribution (Logisim's `SplitterAttributes.computeDistribution`).
pub fn compute_distribution(fanout: usize, bits: usize, ascending: bool) -> Vec<u8> {
    let mut ret = vec![0u8; bits];
    if fanout >= bits {
        for (i, r) in ret.iter_mut().enumerate() {
            *r = if ascending { i + 1 } else { fanout - i } as u8;
        }
        return ret;
    }
    let per_end = bits / fanout;
    let mut extra = bits % fanout;
    let (mut cur, mut left) = (-1i32, 0usize);
    let order: Vec<usize> = if ascending { (0..bits).collect() } else { (0..bits).rev().collect() };
    for i in order {
        if left == 0 {
            cur += 1;
            left = per_end;
            if extra > 0 {
                left += 1;
                extra -= 1;
            }
        }
        ret[i] = (1 + cur) as u8;
        left -= 1;
    }
    ret
}

/// Splits a `width`-bit bus (port 0, at the origin) into `fanout` ends (ports 1..=fanout).
/// Bidirectional: values flow from the bus to the ends and back, bit by bit.
pub struct Splitter {
    opts: SplitterOptions,
}

impl Splitter {
    pub fn new(opts: SplitterOptions) -> Self {
        assert_eq!(opts.bit_end.len(), opts.width as usize);
        Splitter { opts }
    }
    /// Combined-bit indices that go to `end` (1-based), ascending.
    fn bits_of(&self, end: usize) -> Vec<u8> {
        (0..self.opts.width).filter(|&b| self.opts.bit_end[b as usize] as usize == end).collect()
    }
    /// Offset of end `i` (1-based), east-facing (ported from `SplitterParameters`).
    pub fn end_offset(&self, i: usize) -> (i32, i32) {
        let gap = self.opts.spacing * 10;
        let n = self.opts.fanout as i32;
        let dy0 = match self.opts.appearance {
            SplitterAppearance::Center | SplitterAppearance::Legacy => -gap * (n / 2),
            SplitterAppearance::Right => 10,
            SplitterAppearance::Left => -(10 + gap * (n - 1)),
        };
        (20, dy0 + gap * (i as i32 - 1))
    }
}

const APPEARANCES: [(&str, SplitterAppearance); 4] = [
    ("left", SplitterAppearance::Left),
    ("right", SplitterAppearance::Right),
    ("center", SplitterAppearance::Center),
    ("legacy", SplitterAppearance::Legacy),
];

impl Component for Splitter {
    fn name(&self) -> &str {
        "Splitter"
    }
    fn options(&self) -> OptionMap {
        let o = &self.opts;
        let bit_end: Vec<String> = o.bit_end.iter().map(|e| e.to_string()).collect();
        OptionMap::new()
            .with("fanout", o.fanout)
            .with("width", o.width)
            .with("spacing", o.spacing)
            .with("appearance", OptionMap::name_of(o.appearance, &APPEARANCES))
            .with("bit_end", bit_end.join(","))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("fanout", "Fan out", 1, 64, 2),
            OptionSpec::int("width", "Bit width in", 1, 64, 2),
            OptionSpec::int("spacing", "Spacing", 1, 8, 1),
            OptionSpec::choice("appearance", "Appearance", &["left", "right", "center", "legacy"], "left"),
            OptionSpec::string("bit_end", "Bit → end (comma list, 0 = none)", "1,2"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["fanout", "width", "spacing", "appearance", "bit_end"])?;
        let fanout = m.int("fanout", 1, 64)?.map_or(self.opts.fanout, |v| v as usize);
        let width = m.int("width", 1, 64)?.map_or(self.opts.width, |v| v as u8);
        let mut o = SplitterOptions::new(fanout, width);
        if fanout == self.opts.fanout && width == self.opts.width {
            o.bit_end = self.opts.bit_end.clone();
        }
        o.spacing = m.int("spacing", 1, 8)?.map_or(self.opts.spacing, |v| v as i32);
        o.appearance = m.choice("appearance", &APPEARANCES)?.unwrap_or(self.opts.appearance);
        if let Some(s) = m.string("bit_end")? {
            let parsed: Result<Vec<u8>, _> = s.split(',').map(|t| t.trim().parse::<u8>()).collect();
            let be = parsed.map_err(|e| format!("option 'bit_end': {e}"))?;
            if be.len() != width as usize || be.iter().any(|&e| e as usize > fanout) {
                return Err(format!("option 'bit_end' needs {width} entries, each in 0..={fanout}"));
            }
            o.bit_end = be;
        }
        self.opts = o;
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let mut p = vec![Port::new(0, 0, PortKind::InOut, bw(self.opts.width))];
        for e in 1..=self.opts.fanout {
            let (x, y) = self.end_offset(e);
            let w = self.bits_of(e).len().max(1) as u8;
            p.push(Port::new(x, y, PortKind::InOut, bw(w)));
        }
        p
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let width = self.opts.width;
        let take = |v: Value, w: u8| if v.width().get() == w { v } else { Value::unknown(bw(w)) };
        // bus → ends
        let bus = take(ctx.port_value_others(0), width);
        for e in 1..=self.opts.fanout {
            let bits = self.bits_of(e);
            let out = if bits.is_empty() {
                Value::UNKNOWN
            } else {
                Value::from_bits(&bits.iter().map(|&b| bus.bit(b)).collect::<Vec<_>>())
            };
            ctx.set_port(e, out, 1);
        }
        // ends → bus
        let mut bus_bits = vec![Value::UNKNOWN; width as usize];
        for e in 1..=self.opts.fanout {
            let bits = self.bits_of(e);
            if bits.is_empty() {
                continue;
            }
            let v = take(ctx.port_value_others(e), bits.len() as u8);
            for (pos, &b) in bits.iter().enumerate() {
                bus_bits[b as usize] = v.bit(pos as u8);
            }
        }
        ctx.set_port(0, Value::from_bits(&bus_bits), 1);
    }
}

// ---- Tunnel / Pull / Probe -----------------------------------------------------------

/// Joins every tunnel of the same name (within a circuit) into one net.
pub struct Tunnel {
    pub name: String,
    pub width: BitWidth,
}

impl Tunnel {
    pub fn new(name: &str, width: u8) -> Self {
        Tunnel { name: name.to_string(), width: bw(width) }
    }
}

impl Component for Tunnel {
    fn name(&self) -> &str {
        "Tunnel"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("name", self.name.as_str()).with("width", self.width.get())
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::string("name", "Tunnel name", ""),
            OptionSpec::int("width", "Data bits", 1, 64, 1),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["name", "width"])?;
        if let Some(n) = m.string("name")? {
            self.name = n.to_string();
        }
        if let Some(v) = m.int("width", 1, 64)? {
            self.width = BitWidth::new(v as u8);
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::InOut, self.width)]
    }
    fn tunnel_name(&self) -> Option<String> {
        Some(self.name.clone())
    }
    fn propagate(&mut self, _: &mut dyn PropagateCtx) {}
}

/// Pulls floating bits of its net to `TRUE` (pull-up), `FALSE` (pull-down) or `ERROR`.
pub struct PullResistor {
    pub pull: Value,
}

const PULLS: [(&str, Value); 3] = [("down", Value::FALSE), ("up", Value::TRUE), ("error", Value::ERROR)];

impl PullResistor {
    pub fn new(pull: Value) -> Self {
        assert!(pull == Value::TRUE || pull == Value::FALSE || pull == Value::ERROR);
        PullResistor { pull }
    }
}

impl Component for PullResistor {
    fn name(&self) -> &str {
        "Pull Resistor"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("pull", OptionMap::name_of(self.pull, &PULLS))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::choice("pull", "Pull direction", &["down", "up", "error"], "down"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["pull"])?;
        self.pull = m.choice("pull", &PULLS)?.unwrap_or(self.pull);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::InOut, Port::ANY_WIDTH)]
    }
    fn pull_value(&self) -> Option<Value> {
        Some(self.pull)
    }
    fn propagate(&mut self, _: &mut dyn PropagateCtx) {}
}

/// Shows the value of its net; read it through the [`ProbeHandle`].
pub type ProbeHandle = Rc<Cell<Value>>;

pub struct Probe {
    seen: ProbeHandle,
}

impl Probe {
    pub fn new() -> Self {
        Probe { seen: Rc::new(Cell::new(Value::NIL)) }
    }
    pub fn handle(&self) -> ProbeHandle {
        Rc::clone(&self.seen)
    }
}

impl Default for Probe {
    fn default() -> Self {
        Probe::new()
    }
}

impl Component for Probe {
    fn name(&self) -> &str {
        "Probe"
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Input, Port::ANY_WIDTH)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        self.seen.set(ctx.port_value(0));
    }
}

// ---- Constant / Power / Ground ---------------------------------------------------------

pub struct Constant {
    pub width: BitWidth,
    pub value: u64,
}

impl Constant {
    pub fn new(width: u8, value: u64) -> Self {
        Constant { width: bw(width), value }
    }
}

impl Component for Constant {
    fn name(&self) -> &str {
        "Constant"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width.get()).with("value", self.value as i64)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("width", "Data bits", 1, 64, 1),
            OptionSpec::int("value", "Value", i64::MIN, i64::MAX, 1),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width", "value"])?;
        if let Some(v) = m.int("width", 1, 64)? {
            self.width = BitWidth::new(v as u8);
        }
        if let Some(v) = m.int("value", i64::MIN, i64::MAX)? {
            self.value = v as u64;
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, self.width)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        ctx.set_port(0, Value::known(self.width, self.value), 1);
    }
}

/// Drives all-ones.
pub struct Power {
    pub width: BitWidth,
}
impl Power {
    pub fn new(width: u8) -> Self {
        Power { width: bw(width) }
    }
}
impl Component for Power {
    fn name(&self) -> &str {
        "Power"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width.get())
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("width", "Data bits", 1, 64, 1),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width"])?;
        if let Some(v) = m.int("width", 1, 64)? {
            self.width = BitWidth::new(v as u8);
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, self.width)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        ctx.set_port(0, Value::repeat(Value::TRUE, self.width), 1);
    }
}

/// Drives all-zeros.
pub struct Ground {
    pub width: BitWidth,
}
impl Ground {
    pub fn new(width: u8) -> Self {
        Ground { width: bw(width) }
    }
}
impl Component for Ground {
    fn name(&self) -> &str {
        "Ground"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width.get())
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("width", "Data bits", 1, 64, 1),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width"])?;
        if let Some(v) = m.int("width", 1, 64)? {
            self.width = BitWidth::new(v as u8);
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, self.width)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        ctx.set_port(0, Value::repeat(Value::FALSE, self.width), 1);
    }
}

// ---- Transistor / Transmission gate -------------------------------------------------------

/// Bits become errors (or stay floating) when the gate is not fully defined.
fn garble(input: Value, width: BitWidth) -> Value {
    if input.is_fully_defined() {
        return Value::error(width);
    }
    let bits: Vec<Value> =
        input.bits().into_iter().map(|b| if b == Value::UNKNOWN { b } else { Value::ERROR }).collect();
    Value::from_bits(&bits)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransistorType {
    P,
    N,
}

/// Ports: 0 drain (output), 1 source (input), 2 gate. P conducts on 0, N on 1.
pub struct Transistor {
    pub kind: TransistorType,
    pub width: BitWidth,
    /// Gate on the "top right" side instead of top left (Logisim's SELECT_LOC).
    pub select_top_right: bool,
}

const TRANSISTOR_TYPES: [(&str, TransistorType); 2] = [("p", TransistorType::P), ("n", TransistorType::N)];

impl Transistor {
    pub fn new(kind: TransistorType, width: u8) -> Self {
        Transistor { kind, width: bw(width), select_top_right: false }
    }
}

impl Component for Transistor {
    fn name(&self) -> &str {
        "Transistor"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new()
            .with("type", OptionMap::name_of(self.kind, &TRANSISTOR_TYPES))
            .with("width", self.width.get())
            .with("select_top_right", self.select_top_right)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::choice("type", "Type", &["p", "n"], "p"),
            OptionSpec::int("width", "Data bits", 1, 64, 1),
            OptionSpec::boolean("select_top_right", "Gate on top right", false),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["type", "width", "select_top_right"])?;
        self.kind = m.choice("type", &TRANSISTOR_TYPES)?.unwrap_or(self.kind);
        if let Some(v) = m.int("width", 1, 64)? {
            self.width = BitWidth::new(v as u8);
        }
        self.select_top_right = m.boolean("select_top_right")?.unwrap_or(self.select_top_right);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        // east facing: dx = -1, dy = 0
        let gate_y = if self.select_top_right { -20 } else { 20 };
        // flip = (not N/W) == top_right  →  gate sits at y = +20 unless top_right
        vec![
            Port::new(0, 0, PortKind::Output, self.width),
            Port::new(-40, 0, PortKind::Input, self.width),
            Port::new(-20, gate_y, PortKind::Input, BitWidth::ONE),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let (gate, input) = (ctx.port_value(2), ctx.port_value(1));
        let desired = if self.kind == TransistorType::P { Value::FALSE } else { Value::TRUE };
        let out = if !gate.is_fully_defined() {
            garble(input, self.width)
        } else if gate != desired {
            Value::unknown(self.width)
        } else {
            // inputs equal to the conducting level become Z; everything else passes
            let bits: Vec<Value> =
                input.bits().into_iter().map(|b| if b == desired { Value::UNKNOWN } else { b }).collect();
            Value::from_bits(&bits)
        };
        ctx.set_port(0, out, 1);
    }
}

/// Ports: 0 output, 1 input, 2 gate0 (p-gate), 3 gate1 (n-gate). Passes input when
/// gate0 = 0 and gate1 = 1.
pub struct TransmissionGate {
    pub width: BitWidth,
    pub select_top_right: bool,
}

impl TransmissionGate {
    pub fn new(width: u8) -> Self {
        TransmissionGate { width: bw(width), select_top_right: false }
    }
}

impl Component for TransmissionGate {
    fn name(&self) -> &str {
        "Transmission Gate"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width.get()).with("select_top_right", self.select_top_right)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("width", "Data bits", 1, 64, 1),
            OptionSpec::boolean("select_top_right", "Gate on top right", false),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width", "select_top_right"])?;
        if let Some(v) = m.int("width", 1, 64)? {
            self.width = BitWidth::new(v as u8);
        }
        self.select_top_right = m.boolean("select_top_right")?.unwrap_or(self.select_top_right);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let (g0, g1) = if self.select_top_right { (-20, 20) } else { (20, -20) };
        vec![
            Port::new(0, 0, PortKind::Output, self.width),
            Port::new(-40, 0, PortKind::Input, self.width),
            Port::new(-20, g0, PortKind::Input, BitWidth::ONE),
            Port::new(-20, g1, PortKind::Input, BitWidth::ONE),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let (input, g0, g1) = (ctx.port_value(1), ctx.port_value(2), ctx.port_value(3));
        let out = if g0.is_fully_defined() && g1.is_fully_defined() && g0 != g1 {
            if g0 == Value::TRUE {
                Value::unknown(self.width)
            } else {
                input
            }
        } else {
            garble(input, self.width)
        };
        ctx.set_port(0, out, 1);
    }
}

// ---- Bit extender ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtendKind {
    #[default]
    Zero,
    One,
    Sign,
    /// Fill bit comes from a 1-bit input port (port 2).
    Input,
}

const EXTEND_KINDS: [(&str, ExtendKind); 4] = [
    ("zero", ExtendKind::Zero),
    ("one", ExtendKind::One),
    ("sign", ExtendKind::Sign),
    ("input", ExtendKind::Input),
];

pub struct BitExtender {
    pub in_width: BitWidth,
    pub out_width: BitWidth,
    pub kind: ExtendKind,
}

impl BitExtender {
    pub fn new(in_width: u8, out_width: u8, kind: ExtendKind) -> Self {
        BitExtender { in_width: bw(in_width), out_width: bw(out_width), kind }
    }
}

impl Component for BitExtender {
    fn name(&self) -> &str {
        "Bit Extender"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new()
            .with("in_width", self.in_width.get())
            .with("out_width", self.out_width.get())
            .with("kind", OptionMap::name_of(self.kind, &EXTEND_KINDS))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("in_width", "Input bits", 1, 64, 8),
            OptionSpec::int("out_width", "Output bits", 1, 64, 16),
            OptionSpec::choice("kind", "Extension", &["zero", "one", "sign", "input"], "zero"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["in_width", "out_width", "kind"])?;
        if let Some(v) = m.int("in_width", 1, 64)? {
            self.in_width = BitWidth::new(v as u8);
        }
        if let Some(v) = m.int("out_width", 1, 64)? {
            self.out_width = BitWidth::new(v as u8);
        }
        self.kind = m.choice("kind", &EXTEND_KINDS)?.unwrap_or(self.kind);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let mut p = vec![
            Port::new(0, 0, PortKind::Output, self.out_width),
            Port::new(-40, 0, PortKind::Input, self.in_width),
        ];
        if self.kind == ExtendKind::Input {
            p.push(Port::new(-20, -20, PortKind::Input, BitWidth::ONE));
        }
        p
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let input = ctx.port_value(1);
        let fill = match self.kind {
            ExtendKind::One => Value::TRUE,
            ExtendKind::Sign => {
                let w = input.width().get();
                if w > 0 {
                    input.bit(w - 1)
                } else {
                    Value::ERROR
                }
            }
            ExtendKind::Input => {
                let f = ctx.port_value(2);
                if f.width() == BitWidth::ONE {
                    f
                } else {
                    Value::ERROR
                }
            }
            ExtendKind::Zero => Value::FALSE,
        };
        ctx.set_port(0, input.extend_width(self.out_width.get(), fill), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gates::{ControlledBuffer, Gate, GateOptions};
    use logisim_core::{Circuit, Direction, Location};

    fn at(x: i32, y: i32) -> Location {
        Location::new(x, y)
    }
    fn put(c: &mut Circuit, x: i32, y: i32, comp: impl Component + 'static) -> logisim_core::CompId {
        c.add_component(at(x, y), Direction::East, Box::new(comp))
    }
    /// Place `comp` so that its port `port` sits exactly on `p` (ports sharing a point share a net).
    fn put_port_at(c: &mut Circuit, p: Location, port: usize, comp: impl Component + 'static) -> logisim_core::CompId {
        let off = comp.ports()[port].offset;
        c.add_component(at(p.x - off.x, p.y - off.y), Direction::East, Box::new(comp))
    }
    fn w(n: u8) -> BitWidth {
        BitWidth::new(n)
    }

    #[test]
    fn distribution_matches_logisim() {
        assert_eq!(compute_distribution(2, 2, true), vec![1, 2]);
        assert_eq!(compute_distribution(2, 4, true), vec![1, 1, 2, 2]);
        assert_eq!(compute_distribution(3, 8, true), vec![1, 1, 1, 2, 2, 2, 3, 3]);
        assert_eq!(compute_distribution(2, 2, false), vec![2, 1]);
        assert_eq!(compute_distribution(8, 4, true), vec![1, 2, 3, 4]);
    }

    #[test]
    fn splitter_fans_out_and_back_in() {
        // 4-bit bus into a 4→2x2 splitter, probes on both ends
        let mut c = Circuit::new();
        let pin = put(&mut c, 0, 0, Pin::new(Value::known(w(4), 0b1001)));
        let sp = put_port_at(&mut c, at(0, 0), 0, Splitter::new(SplitterOptions::new(2, 4)));
        let _ = (pin, sp);
        let mut probes = vec![];
        for e in 1..=2 {
            let p = Probe::new();
            probes.push(p.handle());
            let loc = c.port_location(sp, e);
            put_port_at(&mut c, loc, 0, p);
        }
        c.reset();
        assert_eq!(probes[0].get().to_u64(), Some(0b01)); // bits 0,1 = 1,0
        assert_eq!(probes[1].get().to_u64(), Some(0b10)); // bits 2,3 = 0,1

        // reverse direction: two 2-bit pins into the ends, probe on the bus
        let mut c = Circuit::new();
        let sp = put(&mut c, 0, 0, Splitter::new(SplitterOptions::new(2, 4)));
        for (e, v) in [(1usize, 0b01u64), (2, 0b10)] {
            let loc = c.port_location(sp, e);
            put_port_at(&mut c, loc, 0, Pin::new(Value::known(w(2), v)));
        }
        let probe = Probe::new();
        let h = probe.handle();
        put_port_at(&mut c, at(0, 0), 0, probe);
        c.reset();
        assert_eq!(h.get().to_u64(), Some(0b1001));
        assert!(!c.is_oscillating());
    }

    #[test]
    fn splitter_unmapped_bit_floats_and_xor_order() {
        let opts = SplitterOptions::new(2, 4).map_bit(1, 0);
        let sp = Splitter::new(opts);
        assert_eq!(sp.ports()[1].width.get(), 1); // end 1 has bit 0 only
        let mut c = Circuit::new();
        let id = put(&mut c, 0, 0, sp);
        let loc1 = c.port_location(id, 1);
        put_port_at(&mut c, loc1, 0, Pin::new(Value::TRUE));
        let probe = Probe::new();
        let h = probe.handle();
        put_port_at(&mut c, at(0, 0), 0, probe);
        c.reset();
        // bit0 = 1, bit1 unmapped = x, bits 2,3 end 2 undriven = x
        assert_eq!(h.get().to_binary_string(), "xxx1");
    }

    #[test]
    fn splitter_end_geometry() {
        let sp = Splitter::new(SplitterOptions::new(3, 3));
        let offs: Vec<_> = sp.ports().iter().map(|p| (p.offset.x, p.offset.y)).collect();
        assert_eq!(offs, vec![(0, 0), (20, -30), (20, -20), (20, -10)]);
    }

    #[test]
    fn tunnels_join_by_name() {
        let mut c = Circuit::new();
        put(&mut c, 0, 0, Pin::bit(true));
        put(&mut c, 0, 0, Tunnel::new("sig", 1));
        let probe = Probe::new();
        let h = probe.handle();
        put(&mut c, 500, 500, probe);
        put(&mut c, 500, 500, Tunnel::new("sig", 1));
        // a differently named tunnel stays separate
        let other = Probe::new();
        let oh = other.handle();
        put(&mut c, 900, 0, other);
        put(&mut c, 900, 0, Tunnel::new("nope", 1));
        c.reset();
        assert_eq!(h.get(), Value::TRUE);
        assert_eq!(oh.get(), Value::UNKNOWN);
    }

    #[test]
    fn pull_resistor_pulls_floating_nets() {
        for (pull, en, data, want) in [
            (Value::TRUE, Value::FALSE, Value::FALSE, Value::TRUE), // floating → pulled up
            (Value::TRUE, Value::TRUE, Value::FALSE, Value::FALSE), // driven wins
            (Value::FALSE, Value::FALSE, Value::TRUE, Value::FALSE),
            (Value::ERROR, Value::FALSE, Value::TRUE, Value::ERROR),
        ] {
            let mut c = Circuit::new();
            let cb = put(&mut c, 100, 0, ControlledBuffer::buffer(1));
            let (inp, ctl) = (c.port_location(cb, 1), c.port_location(cb, 2));
            put_port_at(&mut c, inp, 0, Pin::new(data));
            put_port_at(&mut c, ctl, 0, Pin::new(en));
            put(&mut c, 100, 0, PullResistor::new(pull));
            let probe = Probe::new();
            let h = probe.handle();
            put(&mut c, 100, 0, probe);
            c.reset();
            assert_eq!(h.get(), want, "pull {pull} en {en} data {data}");
        }
    }

    #[test]
    fn opposing_pulls_cancel() {
        let mut c = Circuit::new();
        let cb = put(&mut c, 100, 0, ControlledBuffer::buffer(1));
        let ctl = c.port_location(cb, 2);
        put_port_at(&mut c, ctl, 0, Pin::new(Value::FALSE));
        put(&mut c, 100, 0, PullResistor::new(Value::TRUE));
        put(&mut c, 100, 0, PullResistor::new(Value::FALSE));
        let probe = Probe::new();
        let h = probe.handle();
        put(&mut c, 100, 0, probe);
        c.reset();
        assert_eq!(h.get(), Value::UNKNOWN);
    }

    #[test]
    fn constant_power_ground() {
        let mut c = Circuit::new();
        let mut handles = vec![];
        for (i, comp) in [
            Box::new(Constant::new(8, 0xA5)) as Box<dyn Component>,
            Box::new(Power::new(3)),
            Box::new(Ground::new(3)),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 100 * i as i32;
            c.add_component(at(0, y), Direction::East, comp);
            let p = Probe::new();
            handles.push(p.handle());
            put(&mut c, 0, y, p);
        }
        c.reset();
        assert_eq!(handles[0].get().to_u64(), Some(0xA5));
        assert_eq!(handles[1].get().to_u64(), Some(0b111));
        assert_eq!(handles[2].get().to_u64(), Some(0));
    }

    fn pass_test(make: impl Fn() -> Box<dyn Component>, gates: &[Value], data: Value) -> Value {
        let mut c = Circuit::new();
        let id = c.add_component(at(100, 0), Direction::East, make());
        let n = c.port_location(id, 1);
        put_port_at(&mut c, n, 0, Pin::new(data));
        for (i, g) in gates.iter().enumerate() {
            let l = c.port_location(id, 2 + i);
            put_port_at(&mut c, l, 0, Pin::new(*g));
        }
        let probe = Probe::new();
        let h = probe.handle();
        put(&mut c, 100, 0, probe);
        c.reset();
        h.get()
    }

    #[test]
    fn transistor_p_and_n() {
        let p = || Box::new(Transistor::new(TransistorType::P, 1)) as Box<dyn Component>;
        let n = || Box::new(Transistor::new(TransistorType::N, 1)) as Box<dyn Component>;
        // P conducts when gate = 0; the conducting level (0) becomes Z, 1 passes
        assert_eq!(pass_test(p, &[Value::FALSE], Value::TRUE), Value::TRUE);
        assert_eq!(pass_test(p, &[Value::FALSE], Value::FALSE), Value::UNKNOWN);
        assert_eq!(pass_test(p, &[Value::TRUE], Value::TRUE), Value::UNKNOWN);
        // N conducts when gate = 1; 0 passes, 1 becomes Z
        assert_eq!(pass_test(n, &[Value::TRUE], Value::FALSE), Value::FALSE);
        assert_eq!(pass_test(n, &[Value::TRUE], Value::TRUE), Value::UNKNOWN);
        assert_eq!(pass_test(n, &[Value::FALSE], Value::FALSE), Value::UNKNOWN);
        // undefined gate with a known input → error
        assert_eq!(pass_test(n, &[Value::UNKNOWN], Value::TRUE), Value::ERROR);
    }

    #[test]
    fn transmission_gate() {
        let t = || Box::new(TransmissionGate::new(1)) as Box<dyn Component>;
        assert_eq!(pass_test(t, &[Value::FALSE, Value::TRUE], Value::TRUE), Value::TRUE);
        assert_eq!(pass_test(t, &[Value::TRUE, Value::FALSE], Value::TRUE), Value::UNKNOWN);
        assert_eq!(pass_test(t, &[Value::TRUE, Value::TRUE], Value::TRUE), Value::ERROR);
        assert_eq!(pass_test(t, &[Value::UNKNOWN, Value::TRUE], Value::FALSE), Value::ERROR);
    }

    #[test]
    fn bit_extender_kinds() {
        let run = |kind: ExtendKind, v: u64, fill: Option<Value>| {
            let mut gates = vec![];
            gates.extend(fill);
            pass_test(
                move || Box::new(BitExtender::new(2, 4, kind)),
                &gates,
                Value::known(w(2), v),
            )
            .to_binary_string()
        };
        assert_eq!(run(ExtendKind::Zero, 0b10, None), "0010");
        assert_eq!(run(ExtendKind::One, 0b01, None), "1101");
        assert_eq!(run(ExtendKind::Sign, 0b10, None), "1110");
        assert_eq!(run(ExtendKind::Sign, 0b01, None), "0001");
        assert_eq!(run(ExtendKind::Input, 0b01, Some(Value::TRUE)), "1101");
        assert_eq!(run(ExtendKind::Input, 0b01, Some(Value::FALSE)), "0001");
    }

    #[test]
    fn clock_durations_and_phase() {
        let levels = |mut clk: Clock| {
            let mut out = vec![clk.level()];
            struct Null;
            impl PropagateCtx for Null {
                fn port_value(&self, _: usize) -> Value {
                    Value::NIL
                }
                fn port_value_others(&self, _: usize) -> Value {
                    Value::NIL
                }
                fn is_port_connected(&self, _: usize) -> bool {
                    false
                }
                fn set_port(&mut self, _: usize, _: Value, _: u32) {}
            }
            for _ in 0..5 {
                clk.tick(&mut Null);
                out.push(clk.level());
            }
            out.iter().map(|v| if *v == Value::TRUE { '1' } else { '0' }).collect::<String>()
        };
        assert_eq!(levels(Clock::new(1, 1, 0)), "010101");
        assert_eq!(levels(Clock::new(2, 1, 0)), "011011");
        assert_eq!(levels(Clock::new(1, 1, 1)), "101010");
        // huge durations must not overflow
        let _ = levels(Clock::new(u32::MAX, u32::MAX, i64::MIN));
    }

    #[test]
    fn clock_ticks_through_circuit() {
        let mut c = Circuit::new();
        put(&mut c, 0, 0, Clock::default());
        let probe = Probe::new();
        let h = probe.handle();
        put(&mut c, 0, 0, probe);
        c.reset();
        assert_eq!(h.get(), Value::FALSE);
        c.tick();
        assert_eq!(h.get(), Value::TRUE);
        c.tick();
        assert_eq!(h.get(), Value::FALSE);
    }

    #[test]
    fn every_part_round_trips_its_options() {
        for p in &library().parts {
            let part = (p.make)();
            let saved = part.options();
            let mut fresh = (p.make)();
            fresh.set_options(&saved).unwrap_or_else(|e| panic!("{}: {e}", p.name));
            assert_eq!(fresh.options(), saved, "{}", p.name);
            assert_eq!(fresh.ports(), part.ports(), "{}", p.name);
        }
    }

    #[test]
    fn splitter_and_clock_options() {
        let mut s = Splitter::new(SplitterOptions::default());
        let o = OptionMap::new().with("fanout", 3).with("width", 6).with("appearance", "center").with("bit_end", "1,1,2,2,3,3");
        s.set_options(&o).unwrap();
        assert_eq!(s.ports().len(), 4);
        assert_eq!(s.ports()[1].width.get(), 2);
        assert!(s.set_options(&OptionMap::new().with("fanout", 2).with("width", 2).with("bit_end", "1,5")).is_err());
        let mut c = Clock::default();
        c.set_options(&OptionMap::new().with("high", 3).with("low", 2)).unwrap();
        assert_eq!((c.high, c.low), (3, 2));
    }

    #[test]
    fn output_pin_observes_and_labels_round_trip() {
        let mut c = Circuit::new();
        put(&mut c, 0, 0, Pin::bit(true));
        let out = Pin::output(1);
        let seen = out.seen_handle();
        put(&mut c, 0, 0, out);
        c.reset();
        assert_eq!(seen.get(), Value::TRUE);
        let mut p = Pin::new(Value::FALSE);
        p.set_options(&OptionMap::new().with("output", true).with("label", "q")).unwrap();
        assert!(p.is_output());
        assert_eq!(p.options().string("label"), Ok(Some("q")));
        assert_eq!(p.ports()[0].kind, PortKind::Input);
    }

    #[test]
    fn option_specs_match_options() {
        for p in &library().parts {
            logisim_core::validate_specs(&mut *(p.make)()).unwrap_or_else(|e| panic!("{e}"));
        }
    }

    #[test]
    fn registry_builds_every_part() {
        let lib = library();
        assert_eq!(lib.parts.len(), 14);
        for p in &lib.parts {
            assert_eq!((p.make)().name(), p.name);
        }
    }

    #[test]
    fn splitter_then_gate_bus_roundtrip() {
        // 2-bit bus → splitter → AND gate of the two bits
        let mut c = Circuit::new();
        let sp = put(&mut c, 0, 0, Splitter::new(SplitterOptions::new(2, 2)));
        put_port_at(&mut c, at(0, 0), 0, Pin::new(Value::known(w(2), 0b11)));
        let g = put(&mut c, 200, 0, Gate::and(GateOptions::default()));
        for e in 1..=2 {
            let a = c.port_location(sp, e);
            let b = c.port_location(g, e);
            // splitter end is a 1-bit net: wire it to the gate input
            c.add_wire(a, at(a.x + 10, a.y));
            c.add_wire(at(a.x + 10, a.y), at(a.x + 10, b.y));
            c.add_wire(at(a.x + 10, b.y), b);
        }
        c.reset();
        assert_eq!(c.port_value(g, 0), Value::TRUE);
    }
}
