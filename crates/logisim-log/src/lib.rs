//! Non-GUI parts of Logisim's logging, timing-diagram and test-vector features
//! (ports of `gui.log`, `gui.test`, `data.TestVector` and `circuit.TestVectorEvaluator`).
//!
//! * [`radix`]: how values are displayed and parsed (binary, octal, decimal, hex, float).
//! * [`signal`]: the recorded history of one signal (run-length encoded, optionally bounded).
//! * [`model`]: [`LogModel`], which records signals while a simulation runs (step, real-time and
//!   clocked modes), and [`LogWriter`], which writes the recording to a text log file.
//! * [`testvec`]: the test-vector file format, the evaluator that runs it against a circuit
//!   through a [`Rig`], and a result tracker.
//! * [`rig`]: a [`Rig`] for a simulated [`logisim_core::Circuit`] built from a project file.
//!
//! Nothing here draws anything: a UI reads the signals/results and renders them.

pub mod model;
pub mod radix;
pub mod rig;
pub mod signal;
pub mod testvec;

pub use model::{
    ClockInfo, CycleInfo, Discipline, Granularity, LogModel, LogWriter, Mode, ModelEvent,
};
pub use radix::Radix;
pub use rig::{CircuitRig, ColumnKind};
pub use signal::{Cursor, Signal, SignalInfo, SourceId};
pub use testvec::{LineReport, Rig, TestError, TestRun, TestVector, TestVectorEvaluator};
