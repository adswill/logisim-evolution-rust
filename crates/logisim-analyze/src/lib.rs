//! Combinational analysis: truth tables, Boolean expressions, minimisation, Karnaugh maps and
//! circuit generation. This is the logic behind Logisim Evolution's "Combinational Analysis"
//! window, without any user interface.

pub mod build;
pub mod csv;
pub mod expr;
pub mod from_circuit;
pub mod implicant;
pub mod kmap;
pub mod model;
pub mod parser;
pub mod table;
pub mod text;
pub mod var;
