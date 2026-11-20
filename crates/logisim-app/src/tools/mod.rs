//! Self-contained app tools. Each submodule has one small entry point so the menu code only
//! needs a line or two to open it:
//!
//! * [`stats::StatsWindow`]  — component statistics dialog
//! * [`hexedit::HexEditor`]  — RAM/ROM contents editor window
//! * [`export`]              — render a circuit to SVG / PNG
//! * [`print`]               — print a circuit through the system browser
//!
//! None of them touch `App`; they work on the document types (`ProjectFile`, `ComponentDef`).

// Entry points are wired up from the menus by the editor thread.
#![allow(dead_code)]

pub mod export;
pub mod hexedit;
pub mod print;
pub mod stats;
mod windows;

pub use windows::register;
