//! The logging model (port of `gui.log.Model` and `LogThread`): which signals are recorded, how
//! simulation progress turns into time on the recorded axis, and the log-file text.
//!
//! The model never touches a circuit. Whoever drives the simulation calls
//! [`LogModel::propagation_completed`] after each tick/step/settle and supplies a `fetch`
//! callback that returns the current value of a signal's [`SourceId`].

use std::io::{self, Write};

use logisim_core::Value;

use crate::signal::{Signal, SignalInfo, SourceId};

/// Which clock edge or level a clocked recording samples on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discipline {
    /// Both edges.
    Dual,
    Rising,
    Falling,
    /// While the clock is high.
    High,
    /// While the clock is low.
    Low,
}

/// How progress of the simulation maps to time on the recorded axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Every simulation step takes `gate_delay`, and a settled circuit takes `time_scale`.
    Step,
    /// Wall-clock time, scaled by `time_scale` (time units per second).
    Real,
    Clock(Discipline),
}

/// Whether intermediate (unsettled) states are recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Granularity {
    /// Only settled states.
    #[default]
    Coarse,
    /// Every simulation step.
    Fine,
}

/// High and low durations (in ticks) of a clock, and its phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleInfo {
    pub hi: u32,
    pub lo: u32,
    pub phase: i64,
}

impl Default for CycleInfo {
    fn default() -> Self {
        CycleInfo {
            hi: 1,
            lo: 1,
            phase: 0,
        }
    }
}

impl CycleInfo {
    pub fn ticks(&self) -> u32 {
        self.hi + self.lo
    }
}

/// The signal that clocks a clocked recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockInfo {
    pub source: SourceId,
    pub cycle: CycleInfo,
}

/// What changed in the model since the UI last looked (see [`LogModel::drain_events`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelEvent {
    SignalsReset,
    SignalsExtended,
    FilePropertyChanged,
    SelectionChanged,
    ModeChanged,
    HistoryLimitChanged,
}

/// Returned when a clocked mode is requested but no clock signal is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoClock;

impl std::fmt::Display for NoClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "this circuit has no clock to log against")
    }
}

impl std::error::Error for NoClock {}

pub type Fetch<'a> = &'a mut dyn FnMut(SourceId) -> Value;

#[derive(Debug)]
pub struct LogModel {
    signals: Vec<Signal>,
    time_end: u64,
    mode: Mode,
    granularity: Granularity,
    time_scale: u64,
    gate_delay: u64,
    history_limit: usize,
    clock: Option<ClockInfo>,
    cur_clock_val: Value,
    elapsed_since_trigger: u64,
    last_realtime_update: u64,
    file_enabled: bool,
    file_header: bool,
    file: Option<String>,
    events: Vec<ModelEvent>,
}

const HI: Value = Value::TRUE;
const LO: Value = Value::FALSE;

impl LogModel {
    /// Start recording `infos` (in the given order). With a `clock`, the model starts in clocked
    /// (dual-edge) mode, otherwise in step mode. Defaults match Logisim: time scale 5000, gate
    /// delay 200, history of 400 segments.
    pub fn new(infos: Vec<SignalInfo>, clock: Option<ClockInfo>, fetch: Fetch) -> LogModel {
        let mut m = LogModel {
            signals: vec![],
            time_end: 0,
            mode: if clock.is_some() {
                Mode::Clock(Discipline::Dual)
            } else {
                Mode::Step
            },
            granularity: Granularity::Coarse,
            time_scale: 5000,
            gate_delay: 200,
            history_limit: 400,
            clock,
            cur_clock_val: Value::UNKNOWN,
            elapsed_since_trigger: 0,
            last_realtime_update: 0,
            file_enabled: false,
            file_header: true,
            file: None,
            events: vec![],
        };
        if let Some(c) = m.clock {
            m.cur_clock_val = fetch(c.source);
        }
        let duration = m.initial_duration();
        for info in infos {
            let v = fetch(info.source);
            m.signals
                .push(Signal::new(info, v, duration, 0, m.history_limit));
        }
        m.time_end = duration;
        m
    }

    pub fn drain_events(&mut self) -> Vec<ModelEvent> {
        std::mem::take(&mut self.events)
    }
    fn fire(&mut self, e: ModelEvent) {
        self.events.push(e);
    }

    // ---- accessors ----

    pub fn signals(&self) -> &[Signal] {
        &self.signals
    }
    pub fn signal(&self, idx: usize) -> &Signal {
        &self.signals[idx]
    }
    pub fn signal_count(&self) -> usize {
        self.signals.len()
    }
    pub fn index_of(&self, source: SourceId) -> Option<usize> {
        self.signals.iter().position(|s| s.info.source == source)
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn granularity(&self) -> Granularity {
        self.granularity
    }
    pub fn is_fine(&self) -> bool {
        self.granularity == Granularity::Fine
    }
    pub fn time_scale(&self) -> u64 {
        self.time_scale
    }
    pub fn gate_delay(&self) -> u64 {
        self.gate_delay
    }
    pub fn history_limit(&self) -> usize {
        self.history_limit
    }
    pub fn clock(&self) -> Option<ClockInfo> {
        self.clock
    }
    pub fn end_time(&self) -> u64 {
        self.time_end
    }
    /// Start of the retained data (later than 0 once old history was dropped).
    pub fn start_time(&self) -> u64 {
        self.signals
            .iter()
            .map(Signal::omitted_data_time)
            .max()
            .unwrap_or(0)
    }
    pub fn file_enabled(&self) -> bool {
        self.file_enabled
    }
    pub fn file_header(&self) -> bool {
        self.file_header
    }
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    // ---- selection ----

    /// Add `info` at `idx` (or move it there if already recorded). Returns false if nothing changed.
    pub fn add_or_move(&mut self, info: SignalInfo, idx: usize, fetch: Fetch) -> bool {
        let idx = idx.min(self.signals.len());
        match self.index_of(info.source) {
            None => {
                let v = fetch(info.source);
                let s = Signal::new(
                    info,
                    v,
                    1,
                    self.time_end.saturating_sub(1),
                    self.history_limit,
                );
                self.signals.insert(idx, s);
            }
            Some(i) if i == idx || i + 1 == idx => return false,
            Some(i) => {
                let s = self.signals.remove(i);
                self.signals.insert(if i < idx { idx - 1 } else { idx }, s);
            }
        }
        self.fire(ModelEvent::SelectionChanged);
        true
    }

    pub fn remove(&mut self, idx: usize) {
        if idx < self.signals.len() {
            self.signals.remove(idx);
            self.fire(ModelEvent::SelectionChanged);
        }
    }

    /// Remove the signal recorded for `source`; returns whether it was there.
    pub fn remove_source(&mut self, source: SourceId) -> bool {
        match self.index_of(source) {
            Some(i) => {
                self.remove(i);
                true
            }
            None => false,
        }
    }

    /// Move the signals at `from` so they end up together just before the signal currently at `to`.
    pub fn move_signals(&mut self, from: &[usize], to: usize) {
        let mut from: Vec<usize> = from
            .iter()
            .copied()
            .filter(|&i| i < self.signals.len())
            .collect();
        from.sort_unstable();
        from.dedup();
        let (Some(&a), Some(&b)) = (from.first(), from.last()) else {
            return;
        };
        if a <= to && to <= b && b - a + 1 == from.len() {
            return; // already together at that spot
        }
        let mut to = to;
        let mut moved = vec![];
        for &i in from.iter().rev() {
            if i < to {
                to -= 1;
            }
            moved.push(self.signals.remove(i));
        }
        for s in moved {
            self.signals.insert(to, s);
        }
        self.fire(ModelEvent::SelectionChanged);
    }

    pub fn set_radix(&mut self, idx: usize, radix: crate::radix::Radix) {
        if let Some(s) = self.signals.get_mut(idx) {
            if s.info.radix != radix {
                s.info.radix = radix;
                self.fire(ModelEvent::SelectionChanged);
            }
        }
    }

    /// Rename a signal (e.g. after its label changed in the circuit).
    pub fn rename(&mut self, idx: usize, name: &str) {
        if let Some(s) = self.signals.get_mut(idx) {
            if s.info.name != name {
                s.info.name = name.to_string();
                self.fire(ModelEvent::SelectionChanged);
            }
        }
    }

    pub fn set_history_limit(&mut self, limit: usize) {
        if self.history_limit != limit {
            self.history_limit = limit;
            for s in &mut self.signals {
                s.resize(limit);
            }
            self.fire(ModelEvent::HistoryLimitChanged);
        }
    }

    // ---- modes ----

    pub fn set_step_mode(
        &mut self,
        fine: bool,
        time_scale: u64,
        gate_delay: u64,
        fetch: Fetch,
        now_ns: u64,
    ) {
        let g = if fine {
            Granularity::Fine
        } else {
            Granularity::Coarse
        };
        if self.mode == Mode::Step
            && self.granularity == g
            && self.time_scale == time_scale
            && self.gate_delay == gate_delay
        {
            return;
        }
        self.time_scale = time_scale;
        self.gate_delay = gate_delay;
        self.set_mode(Mode::Step, g, fetch, now_ns);
    }

    pub fn set_real_mode(&mut self, time_scale: u64, fine: bool, fetch: Fetch, now_ns: u64) {
        let g = if fine {
            Granularity::Fine
        } else {
            Granularity::Coarse
        };
        if self.mode == Mode::Real && self.granularity == g && self.time_scale == time_scale {
            return;
        }
        self.time_scale = time_scale;
        self.set_mode(Mode::Real, g, fetch, now_ns);
    }

    /// Record against `clock` (or the already known clock). Errors if there is no clock at all.
    #[allow(clippy::too_many_arguments)] // mirrors Java's setClockMode; each argument is independent
    pub fn set_clock_mode(
        &mut self,
        fine: bool,
        discipline: Discipline,
        time_scale: u64,
        gate_delay: u64,
        clock: Option<ClockInfo>,
        fetch: Fetch,
        now_ns: u64,
    ) -> Result<(), NoClock> {
        if let Some(c) = clock {
            self.clock = Some(c);
        }
        if self.clock.is_none() {
            return Err(NoClock);
        }
        let g = if fine {
            Granularity::Fine
        } else {
            Granularity::Coarse
        };
        if self.mode == Mode::Clock(discipline)
            && self.granularity == g
            && self.time_scale == time_scale
            && self.gate_delay == gate_delay
        {
            return Ok(());
        }
        self.time_scale = time_scale;
        self.gate_delay = gate_delay;
        self.set_mode(Mode::Clock(discipline), g, fetch, now_ns);
        Ok(())
    }

    /// The clock signal went away (e.g. was deleted): clocked modes fall back to step mode.
    pub fn clear_clock(&mut self, fetch: Fetch, now_ns: u64) {
        self.clock = None;
        if matches!(self.mode, Mode::Clock(_)) {
            let (fine, t, d) = (self.is_fine(), self.time_scale, self.gate_delay);
            self.mode = Mode::Real; // force the change
            self.set_step_mode(fine, t, d, fetch, now_ns);
        }
    }

    fn set_mode(&mut self, m: Mode, g: Granularity, fetch: Fetch, now_ns: u64) {
        self.mode = m;
        self.granularity = g;
        self.simulator_reset(fetch, now_ns);
        self.fire(ModelEvent::ModeChanged);
    }

    // ---- recording ----

    fn cycle(&self) -> CycleInfo {
        self.clock.map_or_else(CycleInfo::default, |c| c.cycle)
    }

    fn capture_continuous(&self) -> bool {
        self.is_fine()
            || (self.mode == Mode::Clock(Discipline::High) && self.cur_clock_val == HI)
            || (self.mode == Mode::Clock(Discipline::Low) && self.cur_clock_val == LO)
    }

    fn real_time_event_duration(&self) -> u64 {
        self.gate_delay.min(self.time_scale)
    }

    fn initial_duration(&self) -> u64 {
        match self.mode {
            Mode::Clock(d) => {
                if self.capture_continuous() {
                    return self.gate_delay;
                }
                let cc = self.cycle();
                match d {
                    Discipline::High => u64::from(cc.lo) * self.time_scale,
                    Discipline::Low => u64::from(cc.hi) * self.time_scale,
                    Discipline::Dual => {
                        let ticks = if self.cur_clock_val == Value::FALSE {
                            cc.lo
                        } else {
                            cc.hi
                        };
                        self.time_scale * u64::from(ticks)
                    }
                    _ => self.time_scale * u64::from(cc.ticks()),
                }
            }
            Mode::Step => self.time_scale,
            Mode::Real => self.real_time_event_duration(),
        }
    }

    fn extend_with_old_values(&mut self, duration: u64) {
        for s in &mut self.signals {
            s.extend_duration(duration);
        }
        self.elapsed_since_trigger += duration;
        self.time_end += duration;
        self.fire(ModelEvent::SignalsExtended);
    }

    fn extend_with_new_values(&mut self, duration: u64, fetch: Fetch) {
        for s in &mut self.signals {
            let v = fetch(s.info.source);
            s.extend(Some(v), duration);
        }
        self.elapsed_since_trigger += duration;
        self.time_end += duration;
        self.fire(ModelEvent::SignalsExtended);
    }

    fn replace_with_new_values(&mut self, duration: u64, fetch: Fetch) {
        for s in &mut self.signals {
            let v = fetch(s.info.source);
            s.replace_recent(Some(v), duration);
        }
        self.fire(ModelEvent::SignalsExtended);
    }

    /// Tell the model the simulation made progress. `stepped` = a propagation step ran,
    /// `propagated` = the circuit settled; `now_ns` is a monotonic clock in nanoseconds (only
    /// real-time mode reads it).
    pub fn propagation_completed(
        &mut self,
        _ticked: bool,
        stepped: bool,
        propagated: bool,
        fetch: Fetch,
        now_ns: u64,
    ) {
        if !stepped && !propagated {
            return;
        }
        if self.granularity == Granularity::Coarse && !propagated {
            return;
        }
        match self.mode {
            Mode::Step => {
                let duration = if propagated {
                    self.time_scale
                } else {
                    self.gate_delay
                };
                self.extend_with_new_values(duration, fetch);
            }
            Mode::Real => self.update_real_mode(fetch, now_ns),
            Mode::Clock(d) => self.update_clock_mode(d, fetch),
        }
    }

    fn update_real_mode(&mut self, fetch: Fetch, now_ns: u64) {
        let elapsed = now_ns.saturating_sub(self.last_realtime_update);
        let duration = ((elapsed as f64 * self.time_scale as f64 / 1e9) as u64).max(1);
        let values: Vec<Value> = self.signals.iter().map(|s| fetch(s.info.source)).collect();
        let mut changed = self.time_end == 0;
        for (s, v) in self.signals.iter_mut().zip(&values) {
            if !changed {
                let prev = s.value_at(self.time_end.saturating_sub(1));
                changed = prev.is_none_or(|p| p != *v);
            }
            s.extend_duration(duration);
        }
        let event = if changed {
            self.real_time_event_duration()
        } else {
            0
        };
        if changed {
            for (s, v) in self.signals.iter_mut().zip(&values) {
                s.extend(Some(*v), event);
            }
        }
        self.elapsed_since_trigger += duration + event;
        self.time_end += duration + event;
        self.last_realtime_update = now_ns;
        self.fire(ModelEvent::SignalsExtended);
    }

    fn update_clock_mode(&mut self, d: Discipline, fetch: Fetch) {
        let Some(clock) = self.clock else { return };
        let v = fetch(clock.source);
        let cc = clock.cycle;
        let scale = self.time_scale;
        match d {
            Discipline::High | Discipline::Low => {
                let high = d == Discipline::High;
                let active = u64::from(if high { cc.hi } else { cc.lo }) * scale;
                let stable = u64::from(if high { cc.lo } else { cc.hi }) * scale;
                let in_active_level = (high && v == HI) || (!high && v == LO);
                if in_active_level {
                    if v != self.cur_clock_val {
                        if self.elapsed_since_trigger < active {
                            self.extend_with_old_values(
                                stable.saturating_sub(self.elapsed_since_trigger),
                            );
                        }
                        self.elapsed_since_trigger = 0;
                        self.cur_clock_val = v;
                    }
                    self.extend_with_new_values(self.gate_delay, fetch);
                } else if v != self.cur_clock_val {
                    if self.elapsed_since_trigger < active {
                        self.extend_with_old_values(active - self.elapsed_since_trigger);
                    }
                    self.elapsed_since_trigger = 0;
                    self.cur_clock_val = v;
                    let duration = if self.is_fine() {
                        self.gate_delay
                    } else {
                        stable
                    };
                    self.extend_with_new_values(duration, fetch);
                } else if !self.is_fine() {
                    self.replace_with_new_values(stable, fetch);
                } else {
                    self.extend_with_new_values(self.gate_delay, fetch);
                }
            }
            _ => {
                let dual = d == Discipline::Dual;
                let low = v == Value::FALSE;
                let ticks = if dual {
                    if low {
                        cc.lo
                    } else {
                        cc.hi
                    }
                } else {
                    cc.ticks()
                };
                let prev_ticks = if dual {
                    if low {
                        cc.hi
                    } else {
                        cc.lo
                    }
                } else {
                    cc.ticks()
                };
                let stable = scale * u64::from(ticks);
                let prev = scale * u64::from(prev_ticks);
                let duration = if self.is_fine() {
                    self.gate_delay
                } else {
                    stable
                };
                let triggered = (dual && v != self.cur_clock_val)
                    || (d == Discipline::Rising && v == HI && self.cur_clock_val != HI)
                    || (d == Discipline::Falling && v == LO && self.cur_clock_val != LO);
                self.cur_clock_val = v;
                if triggered {
                    if self.is_fine() && self.elapsed_since_trigger < prev {
                        self.extend_with_old_values(prev - self.elapsed_since_trigger);
                    }
                    self.elapsed_since_trigger = 0;
                    self.extend_with_new_values(duration, fetch);
                } else if !self.is_fine() {
                    self.replace_with_new_values(stable, fetch);
                } else {
                    self.extend_with_new_values(duration, fetch);
                }
            }
        }
    }

    /// The simulator was reset: start every signal over from its current value.
    pub fn simulator_reset(&mut self, fetch: Fetch, now_ns: u64) {
        if let (Mode::Clock(_), Some(c)) = (self.mode, self.clock) {
            self.cur_clock_val = fetch(c.source);
        }
        let duration = self.initial_duration();
        if self.mode == Mode::Real {
            self.last_realtime_update = now_ns;
        }
        for s in &mut self.signals {
            let v = fetch(s.info.source);
            s.reset(v, duration);
        }
        self.elapsed_since_trigger = duration;
        self.time_end = duration;
        self.fire(ModelEvent::SignalsReset);
    }

    // ---- file logging settings ----

    pub fn set_file(&mut self, path: Option<&str>) {
        let new = path.map(str::to_string);
        if self.file != new {
            self.file_enabled = new.is_some();
            self.file = new;
            self.fire(ModelEvent::FilePropertyChanged);
        }
    }
    pub fn set_file_enabled(&mut self, on: bool) {
        if self.file_enabled != on {
            self.file_enabled = on;
            self.fire(ModelEvent::FilePropertyChanged);
        }
    }
    pub fn set_file_header(&mut self, on: bool) {
        if self.file_header != on {
            self.file_header = on;
            self.fire(ModelEvent::FilePropertyChanged);
        }
    }
}

/// Format a duration in the recorded time unit (nanoseconds): `850 ns`, `1.5 µs`, `2.0 ms`, `1.0 s`.
pub fn format_duration(t: u64) -> String {
    if t < 1_000 || !t.is_multiple_of(100) {
        format!("{t} ns")
    } else if t < 1_000_000 || !t.is_multiple_of(100_000) {
        format!("{:.1} µs", t as f64 / 1e3)
    } else if t < 100_000_000 || !t.is_multiple_of(100_000_000) {
        format!("{:.1} ms", t as f64 / 1e6)
    } else {
        format!("{:.1} s", t as f64 / 1e9)
    }
}

/// Writes a model's recording as tab-separated text, remembering how far it got so it can be
/// called again after every update and only writes the new rows (Java `LogThread.writeSignals`).
///
/// ```text
/// # mode: clocked granularity: coarse
/// clk<TAB>Q[3..0]
/// 0<TAB>0000<TAB># 5.0 µs
/// ```
#[derive(Debug, Default)]
pub struct LogWriter {
    time_next_write: u64,
    mode_dirty: bool,
    header_dirty: bool,
    started: bool,
}

impl LogWriter {
    pub fn new() -> Self {
        LogWriter {
            time_next_write: 0,
            mode_dirty: true,
            header_dirty: true,
            started: false,
        }
    }

    /// The model was reset: start over (and write a fresh mode line and header).
    pub fn restart(&mut self) {
        *self = LogWriter::new();
    }
    pub fn mark_header_dirty(&mut self) {
        self.header_dirty = true;
    }
    pub fn mark_mode_dirty(&mut self) {
        self.mode_dirty = true;
    }

    /// Write everything recorded since the previous call.
    pub fn write_new(&mut self, model: &LogModel, out: &mut impl Write) -> io::Result<()> {
        if !self.started {
            *self = LogWriter::new();
            self.started = true;
        }
        if self.mode_dirty {
            let mode = match model.mode() {
                Mode::Step => "step",
                Mode::Real => "real-time",
                Mode::Clock(_) => "clocked",
            };
            let gran = if model.is_fine() { "fine" } else { "coarse" };
            writeln!(out, "# mode: {mode} granularity: {gran}")?;
            self.mode_dirty = false;
        }
        if self.header_dirty {
            if model.file_header() {
                let names: Vec<String> = model.signals().iter().map(Signal::name).collect();
                writeln!(out, "{}", names.join("\t"))?;
            }
            self.header_dirty = false;
        }
        let stop = model.end_time();
        if self.time_next_write < model.start_time() {
            self.time_next_write = model.start_time(); // old rows were dropped from the history
        }
        while self.time_next_write < stop {
            let mut duration = stop - self.time_next_write;
            let mut cells = Vec::with_capacity(model.signal_count());
            for s in model.signals() {
                match s.segment_at(self.time_next_write) {
                    Some((v, remaining)) => {
                        cells.push(s.info.format(&v));
                        duration = duration.min(remaining);
                    }
                    None => {
                        cells.push("-".to_string());
                        if self.time_next_write < s.start_time() {
                            duration = duration.min(s.start_time() - self.time_next_write);
                        }
                    }
                }
            }
            writeln!(out, "{}\t# {}", cells.join("\t"), format_duration(duration))?;
            self.time_next_write += duration;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_core::BitWidth;
    use std::cell::Cell;

    fn bw(n: u8) -> BitWidth {
        BitWidth::new(n)
    }
    fn info(id: usize, name: &str, w: u8) -> SignalInfo {
        SignalInfo::new(SourceId(id), name, w)
    }

    /// A fake simulation: source 0 is a clock, source 1 a counter.
    struct Sim {
        clk: Cell<bool>,
        count: Cell<u64>,
    }
    impl Sim {
        fn new() -> Self {
            Sim {
                clk: Cell::new(false),
                count: Cell::new(0),
            }
        }
        fn fetch(&self) -> impl FnMut(SourceId) -> Value + '_ {
            move |s| match s.0 {
                0 => {
                    if self.clk.get() {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                _ => Value::known(bw(4), self.count.get()),
            }
        }
        fn tick(&self) {
            self.clk.set(!self.clk.get());
            if self.clk.get() {
                self.count.set(self.count.get() + 1);
            }
        }
    }

    fn clocked(sim: &Sim) -> LogModel {
        let clock = ClockInfo {
            source: SourceId(0),
            cycle: CycleInfo::default(),
        };
        LogModel::new(
            vec![info(0, "clk", 1), info(1, "count", 4)],
            Some(clock),
            &mut sim.fetch(),
        )
    }

    fn segs(m: &LogModel, i: usize) -> Vec<(String, u64)> {
        m.signal(i)
            .segments()
            .map(|(v, d)| (v.to_binary_string(), d))
            .collect()
    }

    #[test]
    fn step_mode_uses_gate_delay_and_time_scale() {
        let sim = Sim::new();
        let mut m = LogModel::new(vec![info(1, "count", 4)], None, &mut sim.fetch());
        assert_eq!(m.mode(), Mode::Step);
        assert_eq!(m.end_time(), 5000);
        sim.count.set(1);
        m.propagation_completed(false, true, false, &mut sim.fetch(), 0); // coarse: unsettled step ignored
        assert_eq!(m.end_time(), 5000);
        m.propagation_completed(false, true, true, &mut sim.fetch(), 0);
        assert_eq!(m.end_time(), 10000);
        m.set_step_mode(true, 5000, 200, &mut sim.fetch(), 0); // fine: every step recorded
        sim.count.set(2);
        m.propagation_completed(false, true, false, &mut sim.fetch(), 0);
        assert_eq!(m.end_time(), 5200);
        assert_eq!(
            segs(&m, 0),
            vec![("0001".into(), 5000), ("0010".into(), 200)]
        );
    }

    #[test]
    fn clocked_dual_edge_records_one_row_per_half_period() {
        let sim = Sim::new();
        let mut m = clocked(&sim);
        assert_eq!(m.mode(), Mode::Clock(Discipline::Dual));
        assert_eq!(m.end_time(), 5000);
        for _ in 0..4 {
            sim.tick();
            m.propagation_completed(true, true, true, &mut sim.fetch(), 0);
        }
        assert_eq!(m.end_time(), 25000);
        assert_eq!(
            segs(&m, 1),
            vec![
                ("0000".into(), 5000),
                ("0001".into(), 10000),
                ("0010".into(), 10000)
            ]
        );
        assert_eq!(m.signal(0).len(), 5);
    }

    #[test]
    fn rising_edge_mode_samples_only_on_triggers() {
        let sim = Sim::new();
        let mut m = clocked(&sim);
        m.set_clock_mode(
            false,
            Discipline::Rising,
            1000,
            100,
            None,
            &mut sim.fetch(),
            0,
        )
        .unwrap();
        assert_eq!(m.end_time(), 2000); // one full cycle (hi + lo) per sample
        sim.tick(); // rising: triggered
        m.propagation_completed(true, true, true, &mut sim.fetch(), 0);
        assert_eq!(m.end_time(), 4000);
        sim.tick(); // falling: not a trigger, coarse replaces the most recent sample
        m.propagation_completed(true, true, true, &mut sim.fetch(), 0);
        assert_eq!(m.end_time(), 4000);
        assert_eq!(
            segs(&m, 1),
            vec![("0000".into(), 2000), ("0001".into(), 2000)]
        );
    }

    #[test]
    fn clocked_mode_needs_a_clock() {
        let sim = Sim::new();
        let mut m = LogModel::new(vec![info(1, "count", 4)], None, &mut sim.fetch());
        let r = m.set_clock_mode(
            false,
            Discipline::Dual,
            1000,
            100,
            None,
            &mut sim.fetch(),
            0,
        );
        assert_eq!(r, Err(NoClock));
        assert_eq!(m.mode(), Mode::Step);
        let clock = ClockInfo {
            source: SourceId(0),
            cycle: CycleInfo {
                hi: 2,
                lo: 3,
                phase: 0,
            },
        };
        m.set_clock_mode(
            false,
            Discipline::Dual,
            1000,
            100,
            Some(clock),
            &mut sim.fetch(),
            0,
        )
        .unwrap();
        assert_eq!(m.end_time(), 3000); // clock starts low: `lo` ticks
        m.clear_clock(&mut sim.fetch(), 0);
        assert_eq!(m.mode(), Mode::Step);
    }

    #[test]
    fn real_time_mode_scales_wall_clock() {
        let sim = Sim::new();
        let mut m = LogModel::new(vec![info(1, "count", 4)], None, &mut sim.fetch());
        m.set_real_mode(1_000_000, false, &mut sim.fetch(), 1_000);
        assert_eq!(m.end_time(), 200); // min(gate_delay, time_scale)
        sim.count.set(3);
        m.propagation_completed(false, true, true, &mut sim.fetch(), 1_000 + 500_000_000); // half a second
        assert_eq!(m.end_time(), 200 + 500_000 + 200);
        let before = m.end_time();
        m.propagation_completed(
            false,
            true,
            true,
            &mut sim.fetch(),
            1_000 + 500_000_000 + 1_000_000_000,
        );
        assert_eq!(m.end_time(), before + 1_000_000); // unchanged values only extend
    }

    #[test]
    fn selection_editing_and_events() {
        let sim = Sim::new();
        let mut m = LogModel::new(
            vec![info(0, "a", 1), info(1, "b", 4)],
            None,
            &mut sim.fetch(),
        );
        m.drain_events();
        assert!(m.add_or_move(info(2, "c", 1), 1, &mut sim.fetch()));
        assert_eq!(
            m.signals()
                .iter()
                .map(|s| s.info.name.clone())
                .collect::<Vec<_>>(),
            ["a", "c", "b"]
        );
        assert_eq!(m.signal(1).end_time(), m.end_time()); // new signals are aligned with the others
        assert!(!m.add_or_move(info(2, "c", 1), 1, &mut sim.fetch()));
        m.move_signals(&[0], 3);
        assert_eq!(
            m.signals()
                .iter()
                .map(|s| s.info.name.clone())
                .collect::<Vec<_>>(),
            ["c", "b", "a"]
        );
        assert!(m.remove_source(SourceId(1)));
        assert!(!m.remove_source(SourceId(1)));
        m.set_history_limit(2);
        assert_eq!(m.drain_events().len(), 4); // add, move, remove, history limit
        assert_eq!(m.history_limit(), 2);
    }

    #[test]
    fn writes_log_rows_incrementally() {
        let sim = Sim::new();
        let mut m = clocked(&sim);
        let mut w = LogWriter::new();
        let mut out = Vec::new();
        w.write_new(&m, &mut out).unwrap();
        sim.tick();
        m.propagation_completed(true, true, true, &mut sim.fetch(), 0);
        w.write_new(&m, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "# mode: clocked granularity: coarse\nclk\tcount[3..0]\n0\t0000\t# 5.0 µs\n1\t0001\t# 5.0 µs\n"
        );
        // no header when disabled
        m.set_file_header(false);
        let mut w = LogWriter::new();
        let mut out = Vec::new();
        w.write_new(&m, &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap().lines().count(), 3);
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration(850), "850 ns");
        assert_eq!(format_duration(1500), "1.5 µs");
        assert_eq!(format_duration(1234), "1234 ns");
        assert_eq!(format_duration(2_000_000), "2.0 ms");
        assert_eq!(format_duration(1_000_000_000), "1.0 s");
    }
}
