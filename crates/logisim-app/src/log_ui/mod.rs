//! Logging UI: timing diagram, logging settings/file output, and the test-vector runner.
//!
//! The editor owns one [`LogUi`] and calls [`LogUi::update`] once per frame with a
//! [`LogContext`]; [`LogUi::menu_items`] adds the window toggles to a menu. Nothing else of the
//! editor is needed here.

mod session;
mod vectors;
mod waveform;

use eframe::egui;
use logisim_core::Circuit;
use logisim_format::ProjectFile;
use logisim_log::{Discipline, Radix};
use session::{LogSession, Settings, Timing};
use std::time::Duration;
use vectors::VectorPanel;
use waveform::WaveState;

/// What the logging windows need from the editor each frame.
pub struct LogContext<'a> {
    pub project: &'a ProjectFile,
    /// Name of the circuit being edited and simulated.
    pub circuit: &'a str,
    /// The live simulation of `circuit`, if simulation is on.
    pub sim: Option<&'a mut Circuit>,
    /// Clock ticks since the simulation was built. A smaller number than last frame tells the
    /// recording that the simulation was reset.
    pub ticks: u64,
}

#[derive(Default)]
pub struct LogUi {
    show_wave: bool,
    show_log: bool,
    show_vectors: bool,
    session: Option<(String, LogSession)>,
    wave: WaveState,
    vectors: VectorPanel,
    note: Option<String>,
}

const RADIXES: [(Radix, &str); 6] = [
    (Radix::Binary, "Binary"),
    (Radix::Octal, "Octal"),
    (Radix::Hex, "Hexadecimal"),
    (Radix::DecimalUnsigned, "Decimal (unsigned)"),
    (Radix::DecimalSigned, "Decimal (signed)"),
    (Radix::Float, "Floating point"),
];

fn radix_name(r: Radix) -> &'static str {
    RADIXES
        .iter()
        .find(|(x, _)| *x == r)
        .map_or("?", |(_, n)| n)
}

impl LogUi {
    pub fn new() -> Self {
        LogUi::default()
    }

    /// Window toggles for a menu (e.g. "View" or "Simulate").
    pub fn menu_items(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.show_wave, "Timing diagram");
        ui.checkbox(&mut self.show_log, "Logging settings");
        ui.checkbox(&mut self.show_vectors, "Test vectors");
    }

    /// Record the simulation and draw whichever windows are open.
    pub fn update(&mut self, ctx: &egui::Context, cx: LogContext<'_>) {
        let LogContext {
            project,
            circuit,
            sim,
            ticks,
        } = cx;
        let Some(def) = project.circuit(circuit) else {
            return;
        };
        let wanted = self.show_wave || self.show_log;
        let stale = self
            .session
            .as_ref()
            .is_none_or(|(n, s)| n != circuit || !s.matches(def));
        if wanted && stale {
            let mut sim = sim;
            self.session = Some((
                circuit.to_string(),
                LogSession::new(def, sim.as_deref_mut()),
            ));
            self.wave = WaveState::default();
            self.note = None;
            return self.update_inner(ctx, project, circuit, sim, ticks);
        }
        self.update_inner(ctx, project, circuit, sim, ticks);
    }

    fn update_inner(
        &mut self,
        ctx: &egui::Context,
        project: &ProjectFile,
        circuit: &str,
        mut sim: Option<&mut Circuit>,
        ticks: u64,
    ) {
        if let (Some((_, s)), Some(c)) = (self.session.as_mut(), sim.as_deref_mut()) {
            s.observe(c, ticks);
            if s.settings.timing == Timing::RealTime {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        let live = sim.is_some();

        if self.show_wave {
            let mut open = true;
            egui::Window::new("Timing diagram")
                .open(&mut open)
                .default_size([760.0, 320.0])
                .show(ctx, |ui| match self.session.as_mut() {
                    Some((_, s)) => {
                        if !live {
                            ui.weak("Simulation is off. Turn on Simulate to record signals.");
                        }
                        let w = ui.available_width() - 178.0;
                        waveform::toolbar(ui, &s.model, &mut self.wave, w);
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                waveform::show(ui, &s.model, &mut self.wave);
                            });
                    }
                    None => {
                        ui.label("Nothing recorded yet.");
                    }
                });
            self.show_wave = open;
        }
        if self.show_log {
            let mut open = true;
            let note = &mut self.note;
            let session = self.session.as_mut().map(|(_, s)| s);
            egui::Window::new("Logging settings")
                .open(&mut open)
                .default_size([420.0, 480.0])
                .show(ctx, |ui| {
                    if let Some(s) = session {
                        log_settings(ui, s, sim, note);
                    }
                });
            self.show_log = open;
        }
        if self.show_vectors {
            let mut open = true;
            let vectors = &mut self.vectors;
            egui::Window::new("Test vectors")
                .open(&mut open)
                .default_size([640.0, 420.0])
                .show(ctx, |ui| {
                    vectors.show(ui, project, circuit);
                });
            self.show_vectors = open;
        }
    }
}

fn log_settings(
    ui: &mut egui::Ui,
    s: &mut LogSession,
    mut sim: Option<&mut Circuit>,
    note: &mut Option<String>,
) {
    egui::CollapsingHeader::new("Signals")
        .default_open(true)
        .show(ui, |ui| {
            let available: Vec<_> = s.available().to_vec();
            if available.is_empty() {
                ui.weak("Give pins a label to make them loggable.");
            }
            for info in available {
                let idx = s.model.index_of(info.source);
                ui.horizontal(|ui| {
                    let mut on = idx.is_some();
                    if ui.checkbox(&mut on, info.display_name()).changed() {
                        s.set_recorded(info.source, on, sim.as_deref_mut());
                    }
                    if let Some(i) = idx {
                        let cur = s.model.signal(i).info.radix;
                        egui::ComboBox::from_id_salt(("radix", info.source.0))
                            .selected_text(radix_name(cur))
                            .show_ui(ui, |ui| {
                                for (r, name) in RADIXES {
                                    if ui.selectable_label(cur == r, name).clicked() {
                                        s.model.set_radix(i, r);
                                        s.sync();
                                    }
                                }
                            });
                        if ui
                            .add_enabled(i > 0, egui::Button::new("▲").small())
                            .on_hover_text("Move up")
                            .clicked()
                        {
                            s.model.move_signals(&[i], i - 1);
                            s.sync();
                        }
                        if ui
                            .add_enabled(
                                i + 1 < s.model.signal_count(),
                                egui::Button::new("▼").small(),
                            )
                            .on_hover_text("Move down")
                            .clicked()
                        {
                            s.model.move_signals(&[i], i + 2);
                            s.sync();
                        }
                    }
                });
            }
        });
    ui.separator();
    let mut want: Settings = s.settings;
    ui.horizontal(|ui| {
        ui.label("Time axis:");
        let name = |t: Timing| match t {
            Timing::Step => "Simulation steps",
            Timing::RealTime => "Real time",
            Timing::Clocked(_) => "Clock edges",
        };
        egui::ComboBox::from_id_salt("timing")
            .selected_text(name(want.timing))
            .show_ui(ui, |ui| {
                for t in [
                    Timing::Step,
                    Timing::RealTime,
                    Timing::Clocked(Discipline::Dual),
                ] {
                    let same = std::mem::discriminant(&t) == std::mem::discriminant(&want.timing);
                    if ui.selectable_label(same, name(t)).clicked() && !same {
                        want.timing = t;
                    }
                }
            });
        if let Timing::Clocked(d) = want.timing {
            let dn = |d: Discipline| match d {
                Discipline::Dual => "Both edges",
                Discipline::Rising => "Rising edge",
                Discipline::Falling => "Falling edge",
                Discipline::High => "While high",
                Discipline::Low => "While low",
            };
            egui::ComboBox::from_id_salt("discipline")
                .selected_text(dn(d))
                .show_ui(ui, |ui| {
                    for x in [
                        Discipline::Dual,
                        Discipline::Rising,
                        Discipline::Falling,
                        Discipline::High,
                        Discipline::Low,
                    ] {
                        if ui.selectable_label(d == x, dn(x)).clicked() {
                            want.timing = Timing::Clocked(x);
                        }
                    }
                });
        }
    });
    ui.checkbox(
        &mut want.fine,
        "Record every propagation step (not just settled values)",
    );
    ui.horizontal(|ui| {
        ui.label("Time per tick (ns):");
        ui.add(egui::DragValue::new(&mut want.time_scale).range(1..=1_000_000_000u64));
        ui.label("Gate delay (ns):");
        ui.add(egui::DragValue::new(&mut want.gate_delay).range(1..=1_000_000u64));
    });
    if want != s.settings {
        match s.apply_settings(want, sim.as_deref_mut()) {
            Ok(()) => *note = None,
            Err(e) => *note = Some(format!("{e}. Add a Clock part to use clock edges.")),
        }
    }
    ui.horizontal(|ui| {
        let mut limit = s.model.history_limit();
        ui.label("Keep at most");
        if ui
            .add(egui::DragValue::new(&mut limit).range(10..=1_000_000usize))
            .changed()
        {
            s.model.set_history_limit(limit);
            s.sync();
        }
        ui.label("changes per signal");
        if ui.button("Clear recording").clicked() {
            s.clear(sim);
        }
    });
    if let Some(n) = note {
        ui.colored_label(egui::Color32::from_rgb(220, 120, 40), format!("⚠ {n}"));
    }
    ui.separator();
    ui.strong("Log file");
    let mut enabled = s.model.file_enabled();
    let mut path = s.model.file().unwrap_or("").to_string();
    ui.horizontal(|ui| {
        let edited = ui
            .add(
                egui::TextEdit::singleline(&mut path)
                    .desired_width(240.0)
                    .hint_text("file to write"),
            )
            .lost_focus();
        if ui.button("Choose…").clicked() {
            if let Some(p) = rfd::FileDialog::new().set_file_name("log.txt").save_file() {
                s.set_file(Some(&p.to_string_lossy()));
            }
        } else if edited && path != s.model.file().unwrap_or("") {
            s.set_file(if path.is_empty() { None } else { Some(&path) });
        }
    });
    ui.add_enabled_ui(s.model.file().is_some(), |ui| {
        if ui
            .checkbox(
                &mut enabled,
                "Write new values to the file as they are recorded",
            )
            .changed()
        {
            s.set_file_enabled(enabled);
        }
        let mut header = s.model.file_header();
        if ui
            .checkbox(&mut header, "Start the file with a line of signal names")
            .changed()
        {
            s.model.set_file_header(header);
            s.sync();
        }
    });
    if let Some(e) = &s.file_error {
        ui.colored_label(egui::Color32::from_rgb(220, 70, 70), format!("⚠ {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_format::{CircuitDef, ComponentDef};

    fn project() -> ProjectFile {
        let pin = |x, y, l: &str, out: bool| {
            ComponentDef::new("Wiring", "Pin", x, y)
                .unwrap()
                .option("label", l)
                .option("output", out)
        };
        let mut p = ProjectFile::new("t", "main");
        p.circuits.push(CircuitDef {
            name: "main".into(),
            interface: vec![],
            components: vec![
                pin(0, 0, "a", false),
                pin(50, 0, "q", true),
                ComponentDef::new("Wiring", "Clock", 0, 100).unwrap(),
            ],
            wires: vec![logisim_format::WireDef {
                a: (0, 0),
                b: (50, 0),
            }],
        });
        p
    }

    /// Draw every window for a few frames with a live simulation: catches layout panics.
    #[test]
    fn all_windows_draw_without_panicking() {
        let p = project();
        let mut c = p.instantiate("main").unwrap();
        c.reset();
        let ctx = egui::Context::default();
        let mut ui = LogUi::new();
        ui.show_wave = true;
        ui.show_log = true;
        ui.show_vectors = true;
        ui.vectors.load_text("a q\n0 0\n1 1\n");
        ui.vectors.run(&p, "main");
        for frame in 0..4u64 {
            c.tick();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |root| {
                let ctx = root.ctx().clone();
                let ctx = &ctx;
                ui.update(
                    ctx,
                    LogContext {
                        project: &p,
                        circuit: "main",
                        sim: Some(&mut c),
                        ticks: frame,
                    },
                );
            });
            out.textures_delta.clear();
        }
        let (_, s) = ui.session.as_ref().unwrap();
        assert!(s.model.end_time() > 0);
        // no simulation and a missing circuit are both fine
        let mut out = ctx.run_ui(egui::RawInput::default(), |root| {
            let ctx = root.ctx().clone();
            let ctx = &ctx;
            ui.update(
                ctx,
                LogContext {
                    project: &p,
                    circuit: "main",
                    sim: None,
                    ticks: 0,
                },
            );
            ui.update(
                ctx,
                LogContext {
                    project: &p,
                    circuit: "gone",
                    sim: None,
                    ticks: 0,
                },
            );
        });
        out.textures_delta.clear();
    }
}
