//! The "extra" I/O parts from Logisim: Switch, Two-Way Switch, Slider, Two-Pin LED, Buzzer,
//! Programmable Generator and Matrix Keypad. Ports are east-facing; the UI drives/reads the handles.
//!
//! Not ported: NE555 (analog, wall-clock based), Digital Oscilloscope (a display of the timing model in
//! `logisim-log`), PLA-ROM (editor-centric), Real-Time Clock, Video, Telnet, PortIO and Reptar bus
//! (hardware/board specific).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

use super::matrix::SelectLoc;

fn bw(w: u8) -> BitWidth {
    BitWidth::new(w)
}

// ---- Switch -----------------------------------------------------------------------------------

/// Whether the switch is closed.
pub type SwitchHandle = Rc<Cell<bool>>;

/// A one-pole switch: closed passes the input through, open floats the output.
///
/// Port order: 0 = input at (-20,0); 1 = output at (0,0).
pub struct Switch {
    width: BitWidth,
    closed: SwitchHandle,
}

impl Switch {
    pub fn new(width: u8) -> Self {
        Switch { width: bw(width), closed: Rc::new(Cell::new(false)) }
    }
    pub fn handle(&self) -> SwitchHandle {
        Rc::clone(&self.closed)
    }
}

impl Component for Switch {
    fn name(&self) -> &str {
        "Switch"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width.get())
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![OptionSpec::int("width", "Data bits", 1, 64, 1)]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width"])?;
        if let Some(v) = m.int("width", 1, 64)? {
            self.width = bw(v as u8);
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(-20, 0, PortKind::Input, self.width), Port::new(0, 0, PortKind::Output, self.width)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let v = if self.closed.get() { ctx.port_value(0) } else { Value::unknown(self.width) };
        ctx.set_port(1, v, 1);
    }
}

// ---- Two-way switch ------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TwoWayMode {
    /// One input, routed to one of two outputs (default).
    #[default]
    OneInTwoOut,
    /// Two inputs, one of which reaches the single output.
    TwoInOneOut,
}

const TWO_WAY_MODES: [(&str, TwoWayMode); 2] = [("1in_2out", TwoWayMode::OneInTwoOut), ("2in_1out", TwoWayMode::TwoInOneOut)];

/// True when the switch is thrown to the second contact.
pub type TwoWayHandle = Rc<Cell<bool>>;

/// Two-way (changeover) switch.
///
/// Port order: 0 = common pin at (-20,0) (input) or (20,0) (output); 1 = first contact (±20,-10);
/// 2 = second contact (±20,10).
pub struct TwoWaySwitch {
    mode: TwoWayMode,
    width: BitWidth,
    to_second: TwoWayHandle,
}

impl TwoWaySwitch {
    pub fn new(mode: TwoWayMode, width: u8) -> Self {
        TwoWaySwitch { mode, width: bw(width), to_second: Rc::new(Cell::new(false)) }
    }
    pub fn handle(&self) -> TwoWayHandle {
        Rc::clone(&self.to_second)
    }
}

impl Component for TwoWaySwitch {
    fn name(&self) -> &str {
        "Two-Way Switch"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("mode", OptionMap::name_of(self.mode, &TWO_WAY_MODES)).with("width", self.width.get())
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::choice("mode", "Mode", &["1in_2out", "2in_1out"], "1in_2out"),
            OptionSpec::int("width", "Data bits", 1, 64, 1),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["mode", "width"])?;
        self.mode = m.choice("mode", &TWO_WAY_MODES)?.unwrap_or(self.mode);
        if let Some(v) = m.int("width", 1, 64)? {
            self.width = bw(v as u8);
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let one_in = self.mode == TwoWayMode::OneInTwoOut;
        let (common_x, pair_x) = if one_in { (-20, 20) } else { (20, -20) };
        let (common_kind, pair_kind) = if one_in { (PortKind::Input, PortKind::Output) } else { (PortKind::Output, PortKind::Input) };
        vec![
            Port::new(common_x, 0, common_kind, self.width),
            Port::new(pair_x, -10, pair_kind, self.width),
            Port::new(pair_x, 10, pair_kind, self.width),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let second = self.to_second.get();
        match self.mode {
            TwoWayMode::OneInTwoOut => {
                let input = ctx.port_value(0);
                let off = Value::unknown(self.width);
                ctx.set_port(1, if second { off } else { input }, 1);
                ctx.set_port(2, if second { input } else { off }, 1);
            }
            TwoWayMode::TwoInOneOut => {
                let v = ctx.port_value(if second { 2 } else { 1 });
                ctx.set_port(0, v, 1);
            }
        }
    }
}

// ---- Slider ----------------------------------------------------------------------------------

const SLIDER_BITS: u8 = 8;
const SLIDER_MAX: u32 = (1 << SLIDER_BITS) - 1;

/// Visual slider position, 0..=255 from the left.
pub type SliderHandle = Rc<Cell<u32>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SliderDirection {
    #[default]
    LeftToRight,
    RightToLeft,
}

const SLIDER_DIRS: [(&str, SliderDirection); 2] =
    [("left_to_right", SliderDirection::LeftToRight), ("right_to_left", SliderDirection::RightToLeft)];

/// A slider that outputs a number: the position scaled down to `width` bits (default 8).
///
/// Port order: 0 = output at (0,0).
pub struct Slider {
    width: u8,
    direction: SliderDirection,
    position: SliderHandle,
}

impl Slider {
    pub fn new(width: u8) -> Self {
        assert!((1..=SLIDER_BITS).contains(&width), "slider width must be 1..=8");
        Slider { width, direction: SliderDirection::LeftToRight, position: Rc::new(Cell::new(0)) }
    }
    pub fn handle(&self) -> SliderHandle {
        Rc::clone(&self.position)
    }
    /// Move the slider (clamped to the track).
    pub fn set_position(handle: &SliderHandle, pos: u32) {
        handle.set(pos.min(SLIDER_MAX));
    }
    fn value(&self) -> u64 {
        let pos = self.position.get().min(SLIDER_MAX);
        let complete = if self.direction == SliderDirection::RightToLeft { SLIDER_MAX - pos } else { pos };
        (complete >> (SLIDER_BITS - self.width)) as u64
    }
}

impl Component for Slider {
    fn name(&self) -> &str {
        "Slider"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("width", self.width).with("direction", OptionMap::name_of(self.direction, &SLIDER_DIRS))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("width", "Data bits", 1, SLIDER_BITS as i64, SLIDER_BITS as i64),
            OptionSpec::choice("direction", "Direction", &["left_to_right", "right_to_left"], "left_to_right"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["width", "direction"])?;
        if let Some(v) = m.int("width", 1, SLIDER_BITS as i64)? {
            self.width = v as u8;
        }
        let dir = m.choice("direction", &SLIDER_DIRS)?.unwrap_or(self.direction);
        if dir != self.direction {
            // keep the output value when the direction flips, as Logisim does
            self.position.set(SLIDER_MAX - self.position.get().min(SLIDER_MAX));
            self.direction = dir;
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, bw(self.width))]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        ctx.set_port(0, Value::known(bw(self.width), self.value()), 1);
    }
}

// ---- Two-pin LED -----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TwoPinLedState {
    #[default]
    Off,
    /// Anode high, cathode low.
    Forward,
    /// Bipolar LEDs only: anode low, cathode high.
    Reverse,
}

pub type TwoPinLedHandle = Rc<Cell<TwoPinLedState>>;

/// LED with separate anode and cathode pins; optionally bipolar (lights a second colour in reverse).
///
/// Port order: 0 = anode at (10,0); 1 = cathode at (-10,0).
pub struct TwoPinLed {
    bipolar: bool,
    state: TwoPinLedHandle,
}

impl TwoPinLed {
    pub fn new(bipolar: bool) -> Self {
        TwoPinLed { bipolar, state: Rc::new(Cell::new(TwoPinLedState::Off)) }
    }
    pub fn handle(&self) -> TwoPinLedHandle {
        Rc::clone(&self.state)
    }
}

impl Component for TwoPinLed {
    fn name(&self) -> &str {
        "Two-Pin LED"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("bipolar", self.bipolar)
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![OptionSpec::boolean("bipolar", "Bipolar (second colour in reverse)", false)]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["bipolar"])?;
        self.bipolar = m.boolean("bipolar")?.unwrap_or(self.bipolar);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(10, 0, PortKind::Input, BitWidth::ONE), Port::new(-10, 0, PortKind::Input, BitWidth::ONE)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let (anode, cathode) = (ctx.port_value(0), ctx.port_value(1));
        let forward = anode == Value::TRUE && cathode == Value::FALSE;
        let reverse = anode == Value::FALSE && cathode == Value::TRUE;
        self.state.set(if self.bipolar && reverse {
            TwoPinLedState::Reverse
        } else if forward {
            TwoPinLedState::Forward
        } else {
            TwoPinLedState::Off
        });
    }
}

// ---- Buzzer ------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Waveform {
    #[default]
    Sine,
    Square,
    Triangle,
    Sawtooth,
    Noise,
}

const WAVEFORMS: [(&str, Waveform); 5] = [
    ("sine", Waveform::Sine),
    ("square", Waveform::Square),
    ("triangle", Waveform::Triangle),
    ("sawtooth", Waveform::Sawtooth),
    ("noise", Waveform::Noise),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuzzerChannel {
    #[default]
    Both,
    Left,
    Right,
}

const CHANNELS: [(&str, BuzzerChannel); 3] = [("both", BuzzerChannel::Both), ("left", BuzzerChannel::Left), ("right", BuzzerChannel::Right)];

/// Frequency input unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FreqUnit {
    #[default]
    Hz,
    /// Tenths of a hertz.
    DeciHz,
}

const FREQ_UNITS: [(&str, FreqUnit); 2] = [("hz", FreqUnit::Hz), ("dhz", FreqUnit::DeciHz)];

/// What the buzzer should currently play; the audio backend reads this.
#[derive(Debug, Clone, PartialEq)]
pub struct BuzzerSound {
    pub on: bool,
    /// Frequency in Hz (440 when the input is undefined).
    pub hz: u32,
    pub waveform: Waveform,
    pub channel: BuzzerChannel,
    /// Square-wave duty cycle, 0..=255 (128 when undefined).
    pub pulse_width: u32,
    /// Amplitude 0..=32767 (16383.5 when undefined, i.e. half scale).
    pub volume: f64,
    pub smooth_level: u8,
    pub smooth_width: u8,
}

impl BuzzerSound {
    fn silent() -> Self {
        BuzzerSound {
            on: false,
            hz: 440,
            waveform: Waveform::Sine,
            channel: BuzzerChannel::Both,
            pulse_width: 128,
            volume: 0.5,
            smooth_level: 2,
            smooth_width: 2,
        }
    }
}

pub type BuzzerHandle = Rc<RefCell<BuzzerSound>>;

/// Buzzer: plays a tone while enabled.
///
/// Port order: 0 = frequency (14 bits) at (0,-10); 1 = enable at (0,0); 2 = volume at (0,10);
/// 3 = pulse width (8 bits) at (-20,20) (or (-20,-20) with the select on the top/right).
pub struct Buzzer {
    freq_unit: FreqUnit,
    volume_bits: u8,
    waveform: Waveform,
    channel: BuzzerChannel,
    smooth_level: u8,
    smooth_width: u8,
    select_loc: SelectLoc,
    sound: BuzzerHandle,
}

impl Buzzer {
    pub fn new() -> Self {
        Buzzer {
            freq_unit: FreqUnit::Hz,
            volume_bits: 7,
            waveform: Waveform::Sine,
            channel: BuzzerChannel::Both,
            smooth_level: 2,
            smooth_width: 2,
            select_loc: SelectLoc::BottomLeft,
            sound: Rc::new(RefCell::new(BuzzerSound::silent())),
        }
    }
    pub fn handle(&self) -> BuzzerHandle {
        Rc::clone(&self.sound)
    }
}

impl Default for Buzzer {
    fn default() -> Self {
        Self::new()
    }
}

const SELECT_LOCS: [(&str, SelectLoc); 2] = [("bottom_left", SelectLoc::BottomLeft), ("top_right", SelectLoc::TopRight)];

impl Component for Buzzer {
    fn name(&self) -> &str {
        "Buzzer"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new()
            .with("freq_unit", OptionMap::name_of(self.freq_unit, &FREQ_UNITS))
            .with("volume_bits", self.volume_bits)
            .with("waveform", OptionMap::name_of(self.waveform, &WAVEFORMS))
            .with("channel", OptionMap::name_of(self.channel, &CHANNELS))
            .with("smooth_level", self.smooth_level)
            .with("smooth_width", self.smooth_width)
            .with("select_loc", OptionMap::name_of(self.select_loc, &SELECT_LOCS))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::choice("freq_unit", "Frequency unit", &["hz", "dhz"], "hz"),
            OptionSpec::int("volume_bits", "Volume bits", 1, 32, 7),
            OptionSpec::choice("waveform", "Waveform", &["sine", "square", "triangle", "sawtooth", "noise"], "sine"),
            OptionSpec::choice("channel", "Channel", &["both", "left", "right"], "both"),
            OptionSpec::int("smooth_level", "Smoothing level", 0, 10, 2),
            OptionSpec::int("smooth_width", "Smoothing width", 1, 10, 2),
            OptionSpec::choice("select_loc", "Pulse-width side", &["bottom_left", "top_right"], "bottom_left"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["freq_unit", "volume_bits", "waveform", "channel", "smooth_level", "smooth_width", "select_loc"])?;
        self.freq_unit = m.choice("freq_unit", &FREQ_UNITS)?.unwrap_or(self.freq_unit);
        if let Some(v) = m.int("volume_bits", 1, 32)? {
            self.volume_bits = v as u8;
        }
        self.waveform = m.choice("waveform", &WAVEFORMS)?.unwrap_or(self.waveform);
        self.channel = m.choice("channel", &CHANNELS)?.unwrap_or(self.channel);
        if let Some(v) = m.int("smooth_level", 0, 10)? {
            self.smooth_level = v as u8;
        }
        if let Some(v) = m.int("smooth_width", 1, 10)? {
            self.smooth_width = v as u8;
        }
        self.select_loc = m.choice("select_loc", &SELECT_LOCS)?.unwrap_or(self.select_loc);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let pw_y = if self.select_loc == SelectLoc::TopRight { -20 } else { 20 };
        vec![
            Port::new(0, -10, PortKind::Input, bw(14)),
            Port::new(0, 0, PortKind::Input, BitWidth::ONE),
            Port::new(0, 10, PortKind::Input, bw(self.volume_bits)),
            Port::new(-20, pw_y, PortKind::Input, bw(8)),
        ]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let mut s = self.sound.borrow_mut();
        s.on = ctx.port_value(1) == Value::TRUE;
        s.hz = match ctx.port_value(0).to_u64() {
            Some(f) if self.freq_unit == FreqUnit::DeciHz => (f / 10) as u32,
            Some(f) => f as u32,
            None => 440,
        };
        s.waveform = self.waveform;
        s.channel = self.channel;
        s.pulse_width = ctx.port_value(3).to_u64().map_or(128, |v| v as u32);
        s.smooth_level = self.smooth_level;
        s.smooth_width = self.smooth_width;
        s.volume = match ctx.port_value(2).to_u64() {
            Some(v) => (v as f64) * 32767.0 / ((1u64 << self.volume_bits) as f64 - 1.0),
            None => 0.5,
        };
    }
}

// ---- Programmable generator ---------------------------------------------------------------------------

/// Programmable generator: a clock whose high/low durations are set per state and cycle.
///
/// Port order: 0 = output at (0,0). Programme text (option `contents`): high durations for states
/// 1..n, then low durations for states 1..n, space-separated; `v*k` repeats `v` k times. All
/// durations are ticks, at least 1; an empty programme means 1/1 everywhere.
pub struct ProgrammableGenerator {
    high: Vec<u32>,
    low: Vec<u32>,
    state: usize,
    ticks: u32,
    sending: Value,
}

impl ProgrammableGenerator {
    pub fn new(states: usize) -> Self {
        assert!((1..=32).contains(&states), "1..=32 states");
        ProgrammableGenerator { high: vec![1; states], low: vec![1; states], state: 0, ticks: 0, sending: Value::FALSE }
    }
    pub fn high_durations(&self) -> &[u32] {
        &self.high
    }
    pub fn low_durations(&self) -> &[u32] {
        &self.low
    }

    /// The programme in the compact saved form (empty if every duration is 1).
    pub fn contents(&self) -> String {
        let all: Vec<String> = self.high.iter().chain(self.low.iter()).map(|v| v.to_string()).collect();
        if all.iter().all(|v| v == "1") {
            return String::new();
        }
        let mut out = String::new();
        let mut i = 0;
        while i < all.len() {
            let mut n = 1;
            while i + n < all.len() && all[i + n] == all[i] {
                n += 1;
            }
            if n >= 3 {
                out.push_str(&format!("{}*{} ", all[i], n));
            } else {
                for _ in 0..n {
                    out.push_str(&format!("{} ", all[i]));
                }
            }
            i += n;
        }
        out.trim_end().to_string()
    }

    fn load(&mut self, text: &str) -> Result<(), String> {
        let n = self.high.len();
        let mut values: Vec<u32> = Vec::new();
        for tok in text.split_whitespace() {
            let (v, times) = match tok.split_once('*') {
                Some((v, t)) => (v, t.parse::<usize>().map_err(|_| format!("bad repeat count in '{tok}'"))?),
                None => (tok, 1),
            };
            let v: u32 = v.parse().map_err(|_| format!("'{v}' is not a duration"))?;
            if v < 1 {
                return Err("durations must be at least 1 tick".to_string());
            }
            if values.len() + times > 2 * n {
                return Err(format!("too many values: {n} states need {} durations", 2 * n));
            }
            values.extend(std::iter::repeat_n(v, times));
        }
        self.high = vec![1; n];
        self.low = vec![1; n];
        for (i, v) in values.into_iter().enumerate() {
            if i < n {
                self.high[i] = v;
            } else {
                self.low[i - n] = v;
            }
        }
        Ok(())
    }

    fn advance(&mut self) {
        self.ticks += 1;
        if self.ticks > self.high[self.state] + self.low[self.state] {
            self.ticks = 1;
            self.state = (self.state + 1) % self.high.len();
        }
        self.sending = if self.ticks <= self.high[self.state] { Value::TRUE } else { Value::FALSE };
    }
}

impl Component for ProgrammableGenerator {
    fn name(&self) -> &str {
        "Programmable Generator"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("states", self.high.len() as i64).with("contents", self.contents())
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::int("states", "Number of states", 1, 32, 4),
            OptionSpec::string("contents", "Durations (highs then lows; 'v*k' repeats)", ""),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["states", "contents"])?;
        let old = (self.high.clone(), self.low.clone());
        if let Some(n) = m.int("states", 1, 32)? {
            let n = n as usize;
            self.high.resize(n, 1);
            self.low.resize(n, 1);
            self.state = self.state.min(n - 1);
        }
        if let Some(text) = m.string("contents")? {
            if let Err(e) = self.load(text) {
                (self.high, self.low) = old;
                return Err(e);
            }
        }
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, BitWidth::ONE)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        ctx.set_port(0, self.sending, 1);
    }
    fn tick(&mut self, ctx: &mut dyn PropagateCtx) {
        self.advance();
        ctx.set_port(0, self.sending, 1);
    }
}

// ---- Matrix keypad ---------------------------------------------------------------------------------------

/// Pressed keys as a 16-bit mask; key `row*4 + col` is bit `row*4 + col`.
pub type KeypadHandle = Rc<Cell<u16>>;

/// Keypad body size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeypadSize {
    #[default]
    Normal,
    Large,
}

const KEYPAD_SIZES: [(&str, KeypadSize); 2] = [("normal", KeypadSize::Normal), ("large", KeypadSize::Large)];

/// 4x4 matrix keypad in raw mode: a pressed key connects its row line to its column line.
///
/// Port order: 0..4 = row inputs (x = -20+10*row, y = 30); 4..8 = column outputs (x = 30,
/// y = -20+10*col). Large size moves the edge to 60 and the origin offset to -50.
pub struct MatrixKeypad {
    size: KeypadSize,
    pressed: KeypadHandle,
}

impl MatrixKeypad {
    pub fn new(size: KeypadSize) -> Self {
        MatrixKeypad { size, pressed: Rc::new(Cell::new(0)) }
    }
    pub fn handle(&self) -> KeypadHandle {
        Rc::clone(&self.pressed)
    }
    /// Press or release the key at (`row`, `col`).
    pub fn set_key(handle: &KeypadHandle, row: usize, col: usize, down: bool) {
        let bit = 1u16 << (row * 4 + col);
        handle.set(if down { handle.get() | bit } else { handle.get() & !bit });
    }
}

impl Component for MatrixKeypad {
    fn name(&self) -> &str {
        "Matrix Keypad"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("size", OptionMap::name_of(self.size, &KEYPAD_SIZES))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![OptionSpec::choice("size", "Size", &["normal", "large"], "normal")]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["size"])?;
        self.size = m.choice("size", &KEYPAD_SIZES)?.unwrap_or(self.size);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        let (top_left, edge) = if self.size == KeypadSize::Large { (-60, 60) } else { (-30, 30) };
        let mut ps = Vec::new();
        for row in 0..4 {
            ps.push(Port::new(top_left + (row + 1) * 10, edge, PortKind::Input, BitWidth::ONE));
        }
        for col in 0..4 {
            ps.push(Port::new(edge, top_left + (col + 1) * 10, PortKind::Output, BitWidth::ONE));
        }
        ps
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let pressed = self.pressed.get();
        for col in 0..4usize {
            let mut value = Value::UNKNOWN;
            for row in 0..4usize {
                if pressed >> (row * 4 + col) & 1 == 1 {
                    let row_value = ctx.port_value(row);
                    if value == Value::UNKNOWN {
                        value = row_value;
                    } else if row_value != Value::UNKNOWN && value != row_value {
                        value = Value::ERROR;
                    }
                }
            }
            ctx.set_port(4 + col, value, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::testutil::Rig;

    #[test]
    fn switch_passes_or_floats() {
        let s = Switch::new(4);
        let h = s.handle();
        let w = bw(4);
        let mut r = Rig::new(Box::new(s), &[(0, Value::known(w, 9))]);
        assert_eq!(r.get(1), Value::unknown(w));
        h.set(true);
        r.refresh();
        assert_eq!(r.get(1), Value::known(w, 9));
    }

    #[test]
    fn two_way_switch_both_modes() {
        let s = TwoWaySwitch::new(TwoWayMode::OneInTwoOut, 1);
        let h = s.handle();
        let mut r = Rig::new(Box::new(s), &[(0, Value::TRUE)]);
        assert_eq!((r.get(1), r.get(2)), (Value::TRUE, Value::UNKNOWN));
        h.set(true);
        r.refresh();
        assert_eq!((r.get(1), r.get(2)), (Value::UNKNOWN, Value::TRUE));

        let s = TwoWaySwitch::new(TwoWayMode::TwoInOneOut, 1);
        let h = s.handle();
        let mut r = Rig::new(Box::new(s), &[(1, Value::TRUE), (2, Value::FALSE)]);
        assert_eq!(r.get(0), Value::TRUE);
        h.set(true);
        r.refresh();
        assert_eq!(r.get(0), Value::FALSE);
    }

    #[test]
    fn slider_scales_and_flips() {
        let mut s = Slider::new(8);
        let h = s.handle();
        let mut r;
        Slider::set_position(&h, 300); // clamped
        assert_eq!(h.get(), 255);
        s.set_options(&OptionMap::new().with("width", 4)).unwrap();
        Slider::set_position(&h, 0x70);
        assert_eq!(s.value(), 7);
        s.set_options(&OptionMap::new().with("direction", "right_to_left")).unwrap();
        assert_eq!(s.value(), 7, "flipping the direction keeps the value");
        r = Rig::new(Box::new(s), &[]);
        assert_eq!(r.get(0), Value::known(bw(4), 7));
    }

    #[test]
    fn two_pin_led_states() {
        for (bipolar, a, c, want) in [
            (false, Value::TRUE, Value::FALSE, TwoPinLedState::Forward),
            (false, Value::FALSE, Value::TRUE, TwoPinLedState::Off),
            (true, Value::FALSE, Value::TRUE, TwoPinLedState::Reverse),
            (true, Value::TRUE, Value::TRUE, TwoPinLedState::Off),
        ] {
            let led = TwoPinLed::new(bipolar);
            let h = led.handle();
            let _r = Rig::new(Box::new(led), &[(0, a), (1, c)]);
            assert_eq!(h.get(), want);
        }
    }

    #[test]
    fn buzzer_decodes_inputs() {
        let mut b = Buzzer::new();
        b.set_options(&OptionMap::new().with("freq_unit", "dhz").with("waveform", "square")).unwrap();
        let h = b.handle();
        let mut r = Rig::new(
            Box::new(b),
            &[(0, Value::known(bw(14), 4400)), (1, Value::TRUE), (2, Value::known(bw(7), 127)), (3, Value::known(bw(8), 64))],
        );
        {
            let s = h.borrow();
            assert!(s.on);
            assert_eq!((s.hz, s.pulse_width, s.waveform), (440, 64, Waveform::Square));
            assert!((s.volume - 32767.0).abs() < 1e-6);
        }
        r.set(1, Value::FALSE);
        assert!(!h.borrow().on);
        r.set(0, Value::unknown(bw(14)));
        assert_eq!(h.borrow().hz, 440);
    }

    #[test]
    fn generator_cycles_through_states() {
        let mut g = ProgrammableGenerator::new(2);
        g.set_options(&OptionMap::new().with("contents", "2 1 1 3")).unwrap(); // highs 2,1  lows 1,3
        assert_eq!((g.high_durations(), g.low_durations()), (&[2, 1][..], &[1, 3][..]));
        let mut r = Rig::new(Box::new(g), &[]);
        let mut seen = Vec::new();
        for _ in 0..12 {
            r.tick();
            seen.push(r.get(0) == Value::TRUE);
        }
        // state 0: H H L (3 ticks) then state 1: H L L L (4 ticks), repeating
        let expect = [true, true, false, true, false, false, false, true, true, false, true, false];
        assert_eq!(seen, expect);
    }

    #[test]
    fn generator_contents_round_trip_and_errors() {
        let mut g = ProgrammableGenerator::new(4);
        g.set_options(&OptionMap::new().with("contents", "5*4 2 7*3")).unwrap();
        assert_eq!(g.high_durations(), [5, 5, 5, 5]);
        assert_eq!(g.low_durations(), [2, 7, 7, 7]);
        let text = g.contents();
        let mut h = ProgrammableGenerator::new(4);
        h.set_options(&OptionMap::new().with("contents", text.as_str())).unwrap();
        assert_eq!((h.high_durations(), h.low_durations()), (g.high_durations(), g.low_durations()));
        assert_eq!(ProgrammableGenerator::new(4).contents(), "");
        for bad in ["0", "x", "1*9999 1", "1 2 3 4 5 6 7 8 9"] {
            assert!(g.set_options(&OptionMap::new().with("contents", bad)).is_err(), "{bad}");
        }
        assert_eq!(g.high_durations(), [5, 5, 5, 5], "a failed load leaves the programme intact");
    }

    #[test]
    fn keypad_connects_row_to_column() {
        let k = MatrixKeypad::new(KeypadSize::Normal);
        let h = k.handle();
        // drive row 1 low, others unknown
        let mut r = Rig::new(Box::new(k), &[(1, Value::FALSE)]);
        assert!((4..8).all(|p| r.get(p) == Value::UNKNOWN));
        MatrixKeypad::set_key(&h, 1, 2, true);
        r.refresh();
        assert_eq!(r.get(6), Value::FALSE);
        assert_eq!(r.get(4), Value::UNKNOWN);
        MatrixKeypad::set_key(&h, 1, 2, false);
        r.refresh();
        assert_eq!(r.get(6), Value::UNKNOWN);
    }

    #[test]
    fn keypad_conflicting_rows_error() {
        let k = MatrixKeypad::new(KeypadSize::Large);
        let h = k.handle();
        let mut r = Rig::new(Box::new(k), &[(0, Value::TRUE), (3, Value::FALSE)]);
        MatrixKeypad::set_key(&h, 0, 1, true);
        MatrixKeypad::set_key(&h, 3, 1, true);
        r.refresh();
        assert_eq!(r.get(5), Value::ERROR);
        let ports = MatrixKeypad::new(KeypadSize::Large).ports();
        assert_eq!((ports[0].offset.x, ports[0].offset.y, ports[4].offset.x, ports[4].offset.y), (-50, 60, 60, -50));
    }
}
