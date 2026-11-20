//! The non-drawing half of the logging window: which signals a circuit offers, the recording
//! (a [`LogModel`]) and the optional log file. Everything here is testable without a window.

use logisim_core::{BitWidth, Circuit, CompId, Value};
use logisim_format::{CircuitDef, ComponentDef};
use logisim_log::model::NoClock;
use logisim_log::{
    ClockInfo, CycleInfo, Discipline, LogModel, LogWriter, Mode, ModelEvent, SignalInfo, SourceId,
};
use std::fs::File;
use std::time::Instant;

fn is_part(c: &ComponentDef, part: &str) -> bool {
    c.library == "Wiring" && c.part == part
}

fn label(c: &ComponentDef) -> &str {
    c.options.string("label").ok().flatten().unwrap_or("")
}

fn width(c: &ComponentDef) -> u8 {
    c.options.int("width", 1, 64).ok().flatten().unwrap_or(1) as u8
}

/// What can be logged in a circuit: its labelled pins and (the first) clock.
#[derive(Debug, Clone, PartialEq)]
pub struct Sources {
    pub signals: Vec<SignalInfo>,
    pub clock: Option<ClockInfo>,
}

impl Sources {
    /// A source's id is its component's index in the circuit definition, which is also how
    /// the simulator numbers components.
    pub fn of(def: &CircuitDef) -> Sources {
        let mut signals = vec![];
        let mut clock = None;
        for (i, c) in def.components.iter().enumerate() {
            if is_part(c, "Pin") && !label(c).is_empty() {
                let output = c.options.boolean("output").ok().flatten().unwrap_or(false);
                signals.push(SignalInfo::new(SourceId(i), label(c), width(c)).input(!output));
            } else if is_part(c, "Clock") && clock.is_none() {
                signals.push(SignalInfo::new(SourceId(i), "clk", 1).input(true));
                let get = |k: &str, d: i64| {
                    c.options
                        .int(k, i64::MIN, i64::MAX)
                        .ok()
                        .flatten()
                        .unwrap_or(d)
                };
                clock = Some(ClockInfo {
                    source: SourceId(i),
                    cycle: CycleInfo {
                        hi: get("high", 1).max(1) as u32,
                        lo: get("low", 1).max(1) as u32,
                        phase: get("phase", 0),
                    },
                });
            }
        }
        Sources { signals, clock }
    }
}

/// How the time axis advances (the part of [`Mode`] the user picks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    Step,
    RealTime,
    Clocked(Discipline),
}

impl Timing {
    pub fn of(mode: Mode) -> Timing {
        match mode {
            Mode::Step => Timing::Step,
            Mode::Real => Timing::RealTime,
            Mode::Clock(d) => Timing::Clocked(d),
        }
    }
}

pub struct LogSession {
    sources: Sources,
    pub model: LogModel,
    writer: LogWriter,
    file: Option<File>,
    pub file_error: Option<String>,
    last_ticks: u64,
    last_values: Vec<Value>,
    started: Instant,
    pub settings: Settings,
}

/// Recording settings the user can change (applied with [`LogSession::apply_settings`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub timing: Timing,
    pub fine: bool,
    pub time_scale: u64,
    pub gate_delay: u64,
}

fn fetch_from<'a>(
    circuit: Option<&'a mut Circuit>,
    widths: &'a [(usize, u8)],
) -> impl FnMut(SourceId) -> Value + 'a {
    let mut circuit = circuit;
    move |s: SourceId| match circuit.as_mut() {
        Some(c) => c.port_value(CompId(s.0), 0),
        None => {
            let w = widths
                .iter()
                .find(|(i, _)| *i == s.0)
                .map_or(1, |(_, w)| *w);
            Value::unknown(BitWidth::new(w))
        }
    }
}

impl LogSession {
    /// Start recording every signal of `def`. Without a running `circuit`, values are unknown.
    pub fn new(def: &CircuitDef, circuit: Option<&mut Circuit>) -> LogSession {
        let sources = Sources::of(def);
        let widths: Vec<(usize, u8)> = sources
            .signals
            .iter()
            .map(|s| (s.source.0, s.width.get()))
            .collect();
        let model = LogModel::new(
            sources.signals.clone(),
            sources.clock,
            &mut fetch_from(circuit, &widths),
        );
        let settings = Settings {
            timing: Timing::of(model.mode()),
            fine: model.is_fine(),
            time_scale: model.time_scale(),
            gate_delay: model.gate_delay(),
        };
        LogSession {
            sources,
            model,
            writer: LogWriter::new(),
            file: None,
            file_error: None,
            last_ticks: 0,
            last_values: vec![],
            started: Instant::now(),
            settings,
        }
    }

    /// True while `def` still offers exactly the signals this session was built for.
    pub fn matches(&self, def: &CircuitDef) -> bool {
        self.sources == Sources::of(def)
    }

    /// Every signal the circuit offers (recorded or not).
    pub fn available(&self) -> &[SignalInfo] {
        &self.sources.signals
    }

    fn widths(&self) -> Vec<(usize, u8)> {
        self.sources
            .signals
            .iter()
            .map(|s| (s.source.0, s.width.get()))
            .collect()
    }

    fn now_ns(&self) -> u64 {
        self.started.elapsed().as_nanos() as u64
    }

    /// Record what the simulation did since the last call. `ticks` counts clock ticks since
    /// the simulation was built; a smaller value than before means it was rebuilt or reset.
    pub fn observe(&mut self, circuit: &mut Circuit, ticks: u64) {
        let now = self.now_ns();
        let ids: Vec<SourceId> = self.model.signals().iter().map(|s| s.info.source).collect();
        let values: Vec<Value> = ids
            .iter()
            .map(|s| circuit.port_value(CompId(s.0), 0))
            .collect();
        if ticks < self.last_ticks {
            self.model
                .simulator_reset(&mut |s| circuit.port_value(CompId(s.0), 0), now);
            self.rewind_file();
        } else if ticks != self.last_ticks
            || values != self.last_values
            || self.model.mode() == Mode::Real
        {
            let ticked = ticks != self.last_ticks;
            self.model.propagation_completed(
                ticked,
                true,
                true,
                &mut |s| circuit.port_value(CompId(s.0), 0),
                now,
            );
        }
        self.last_ticks = ticks;
        self.last_values = values;
        self.after_change();
    }

    /// Apply new timing settings. Fails (leaving the recording alone) if a clocked mode is
    /// asked for in a circuit without a clock.
    pub fn apply_settings(
        &mut self,
        s: Settings,
        circuit: Option<&mut Circuit>,
    ) -> Result<(), NoClock> {
        let widths = self.widths();
        let now = self.now_ns();
        let mut fetch = fetch_from(circuit, &widths);
        let m = &mut self.model;
        match s.timing {
            Timing::Step => m.set_step_mode(s.fine, s.time_scale, s.gate_delay, &mut fetch, now),
            Timing::RealTime => m.set_real_mode(s.time_scale, s.fine, &mut fetch, now),
            Timing::Clocked(d) => m.set_clock_mode(
                s.fine,
                d,
                s.time_scale,
                s.gate_delay,
                self.sources.clock,
                &mut fetch,
                now,
            )?,
        }
        self.settings = s;
        self.after_change();
        Ok(())
    }

    /// Start over from the current values (the "clear" button).
    pub fn clear(&mut self, circuit: Option<&mut Circuit>) {
        let widths = self.widths();
        let now = self.now_ns();
        self.model
            .simulator_reset(&mut fetch_from(circuit, &widths), now);
        self.rewind_file();
        self.after_change();
    }

    /// Include or drop a signal from the recording.
    pub fn set_recorded(&mut self, source: SourceId, on: bool, circuit: Option<&mut Circuit>) {
        let widths = self.widths();
        if on {
            if self.model.index_of(source).is_none() {
                if let Some(info) = self.sources.signals.iter().find(|s| s.source == source) {
                    let end = self.model.signal_count();
                    self.model
                        .add_or_move(info.clone(), end, &mut fetch_from(circuit, &widths));
                }
            }
        } else {
            self.model.remove_source(source);
        }
        self.after_change();
    }

    /// Log to `path` (or stop with `None`). Creates/truncates the file.
    pub fn set_file(&mut self, path: Option<&str>) {
        self.model.set_file(path);
        self.after_change();
    }

    pub fn set_file_enabled(&mut self, on: bool) {
        self.model.set_file_enabled(on);
        self.after_change();
    }

    fn rewind_file(&mut self) {
        self.writer.restart();
        self.file = None;
        self.file_error = None;
        if let (true, Some(path)) = (self.model.file_enabled(), self.model.file()) {
            match File::create(path) {
                Ok(f) => self.file = Some(f),
                Err(e) => self.file_error = Some(format!("Cannot write {path}: {e}")),
            }
        }
    }

    /// React to model events and write new rows to the log file. Call after changing the
    /// model directly (radix, order, history limit).
    pub fn sync(&mut self) {
        self.after_change();
    }

    fn after_change(&mut self) {
        for e in self.model.drain_events() {
            match e {
                ModelEvent::SelectionChanged => self.writer.mark_header_dirty(),
                ModelEvent::ModeChanged => self.writer.mark_mode_dirty(),
                ModelEvent::SignalsReset => self.writer.restart(),
                ModelEvent::FilePropertyChanged => self.rewind_file(),
                ModelEvent::SignalsExtended | ModelEvent::HistoryLimitChanged => {}
            }
        }
        if let Some(f) = self.file.as_mut() {
            if let Err(e) = self.writer.write_new(&self.model, f) {
                self.file_error = Some(format!("Cannot write log file: {e}"));
                self.file = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_format::ProjectFile;

    /// An input pin `a`, an output pin `q` wired to it, and a clock.
    fn project() -> ProjectFile {
        let pin = |x, y, label: &str, output: bool| {
            ComponentDef::new("Wiring", "Pin", x, y)
                .unwrap()
                .option("label", label)
                .option("output", output)
        };
        let mut p = ProjectFile::new("t", "main");
        p.circuits.push(CircuitDef {
            name: "main".into(),
            interface: vec![],
            components: vec![
                pin(0, 0, "a", false),
                pin(50, 0, "q", true),
                pin(0, 100, "", false), // unlabelled: not offered
                ComponentDef::new("Wiring", "Clock", 0, 200).unwrap(),
            ],
            wires: vec![logisim_format::WireDef {
                a: (0, 0),
                b: (50, 0),
            }],
        });
        p
    }

    fn poke(c: &mut Circuit, v: i64) {
        let opts = logisim_core::OptionMap::new().with("value", v);
        c.component_mut(CompId(0)).set_options(&opts).unwrap();
        c.refresh(CompId(0));
    }

    #[test]
    fn offers_labelled_pins_and_the_clock() {
        let p = project();
        let s = Sources::of(&p.circuits[0]);
        let names: Vec<_> = s
            .signals
            .iter()
            .map(|s| (s.name.as_str(), s.is_input))
            .collect();
        assert_eq!(names, [("a", true), ("q", false), ("clk", true)]);
        assert_eq!(s.clock.unwrap().source, SourceId(3));
    }

    #[test]
    fn observing_records_changes_and_resets() {
        let p = project();
        let def = &p.circuits[0];
        let mut c = p.instantiate("main").unwrap();
        c.reset();
        let mut s = LogSession::new(def, Some(&mut c));
        s.apply_settings(
            Settings {
                timing: Timing::Step,
                ..s.settings
            },
            Some(&mut c),
        )
        .unwrap();
        let t0 = s.model.end_time();
        s.observe(&mut c, 0); // first look: values equal nothing seen before, so a row is added
        let t1 = s.model.end_time();
        assert!(t1 > t0);
        s.observe(&mut c, 0); // nothing changed: nothing recorded
        assert_eq!(s.model.end_time(), t1);
        poke(&mut c, 1);
        s.observe(&mut c, 0);
        assert!(s.model.end_time() > t1);
        let q = s.model.index_of(SourceId(1)).unwrap();
        assert_eq!(s.model.signal(q).segments().last().unwrap().0, Value::TRUE);
        s.observe(&mut c, 5);
        s.observe(&mut c, 1); // ticks went backwards: the simulation restarted
        assert_eq!(s.model.end_time(), s.model.signal(0).end_time());
        assert_eq!(s.model.signal(0).len(), 1);
    }

    #[test]
    fn clocked_mode_needs_a_clock_and_signals_can_be_dropped() {
        let mut p = project();
        let mut s = LogSession::new(&p.circuits[0], None);
        s.set_recorded(SourceId(1), false, None);
        assert_eq!(s.model.signal_count(), 2);
        s.set_recorded(SourceId(1), true, None);
        assert_eq!(s.model.signal(2).info.name, "q"); // re-added at the end
        assert!(!s.matches(&{
            p.circuits[0].components.remove(3);
            p.circuits[0].clone()
        }));
        let mut s = LogSession::new(&p.circuits[0], None);
        let want = Settings {
            timing: Timing::Clocked(Discipline::Rising),
            ..s.settings
        };
        assert_eq!(s.apply_settings(want, None), Err(NoClock));
        assert_eq!(s.settings.timing, Timing::Step);
    }

    #[test]
    fn writes_the_log_file() {
        let p = project();
        let mut c = p.instantiate("main").unwrap();
        c.reset();
        let mut s = LogSession::new(&p.circuits[0], Some(&mut c));
        let path = std::env::temp_dir().join(format!("rcirc-log-test-{}.txt", std::process::id()));
        s.set_file(path.to_str());
        s.observe(&mut c, 0);
        poke(&mut c, 1);
        c.tick(); // a clocked recording adds a row per clock edge
        s.observe(&mut c, 1);
        c.tick();
        s.observe(&mut c, 2);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert!(text.starts_with("# mode:"), "{text}");
        assert!(text.contains("a\tq\tclk"), "{text}");
        assert!(text.lines().count() >= 4, "{text}");
        assert!(s.file_error.is_none());
    }
}
