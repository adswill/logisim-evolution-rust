//! `AppWindow` adapters: the three logging windows share one [`LogUi`].

use super::LogUi;
use crate::windows::{AppWindow, WindowEnv, WindowGroup};
use eframe::egui;
use std::cell::RefCell;
use std::rc::Rc;

type Shared = Rc<RefCell<LogUi>>;

/// Register the timing diagram, logging settings and test vectors windows.
pub fn register(reg: &mut Vec<Box<dyn AppWindow>>) {
    let shared: Shared = Rc::new(RefCell::new(LogUi::new()));
    reg.push(Box::new(Timing(Rc::clone(&shared))));
    reg.push(Box::new(Settings(Rc::clone(&shared))));
    reg.push(Box::new(Vectors(shared)));
}

/// Sample the simulation once per frame, whichever recording window is open.
fn observe(ui: &Shared, env: &mut WindowEnv) {
    let mut l = ui.borrow_mut();
    if l.last_observed == env.frame {
        return;
    }
    l.last_observed = env.frame;
    let ticks = env.sim.as_ref().map_or(0, |s| s.ticks);
    let name = env.editor.current.clone();
    let sim = env.sim.as_mut().map(|s| &mut s.circuit);
    l.observe(env.ctx, &env.editor.project, &name, sim, ticks);
}

struct Timing(Shared);
struct Settings(Shared);
struct Vectors(Shared);

impl AppWindow for Timing {
    fn id(&self) -> &'static str {
        "timing_diagram"
    }
    fn title(&self) -> &str {
        "Timing diagram"
    }
    fn group(&self) -> WindowGroup {
        WindowGroup::Simulate
    }
    fn description(&self) -> &str {
        "Waveforms of the logged signals over time."
    }
    fn default_size(&self) -> [f32; 2] {
        [760.0, 320.0]
    }
    fn update(&mut self, env: &mut WindowEnv) {
        observe(&self.0, env);
    }
    fn show(&mut self, ui: &mut egui::Ui, env: &mut WindowEnv) {
        self.0.borrow_mut().timing_ui(ui, env.sim.is_some());
    }
}

impl AppWindow for Settings {
    fn id(&self) -> &'static str {
        "logging_settings"
    }
    fn title(&self) -> &str {
        "Logging settings"
    }
    fn group(&self) -> WindowGroup {
        WindowGroup::Simulate
    }
    fn description(&self) -> &str {
        "Choose which signals to record, the time axis and an optional log file."
    }
    fn default_size(&self) -> [f32; 2] {
        [420.0, 480.0]
    }
    fn update(&mut self, env: &mut WindowEnv) {
        observe(&self.0, env);
    }
    fn show(&mut self, ui: &mut egui::Ui, env: &mut WindowEnv) {
        let sim = env.sim.as_mut().map(|s| &mut s.circuit);
        self.0.borrow_mut().settings_ui(ui, sim);
    }
}

impl AppWindow for Vectors {
    fn id(&self) -> &'static str {
        "test_vectors"
    }
    fn title(&self) -> &str {
        "Test vectors"
    }
    fn group(&self) -> WindowGroup {
        WindowGroup::Simulate
    }
    fn description(&self) -> &str {
        "Run a table of input and expected output values against the circuit."
    }
    fn default_size(&self) -> [f32; 2] {
        [640.0, 420.0]
    }
    fn show(&mut self, ui: &mut egui::Ui, env: &mut WindowEnv) {
        let name = env.editor.current.clone();
        self.0
            .borrow_mut()
            .vectors_ui(ui, &env.editor.project, &name);
    }
}
