//! The `.rcirc` project format: a versioned RON document describing circuits.
//!
//! A [`ProjectFile`] is plain data (what an editor loads, edits and saves).
//! [`ProjectFile::instantiate`] turns a named circuit into a simulatable
//! [`Circuit`], resolving parts through the component registry and subcircuits by name.

use logisim_components::all_libraries;
use logisim_core::{
    BitWidth, Circuit, Component, Direction, Location, OptVal, OptionMap, SubPortSpec, Subcircuit,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Current format version. Files with a higher version are rejected.
pub const FORMAT_VERSION: u32 = 1;
/// Library id used by [`ComponentDef`]s that instantiate another circuit of the project.
pub const SUBCIRCUIT_LIBRARY: &str = "Circuit";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub format_version: u32,
    pub metadata: Metadata,
    pub circuits: Vec<CircuitDef>,
    /// Name of the main circuit (must exist in `circuits`).
    pub main: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub generator: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircuitDef {
    pub name: String,
    /// Ports other circuits see when they instantiate this one.
    #[serde(default)]
    pub interface: Vec<InterfaceDef>,
    pub components: Vec<ComponentDef>,
    #[serde(default)]
    pub wires: Vec<WireDef>,
}

/// One port of a circuit used as a subcircuit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceDef {
    pub name: String,
    pub input: bool,
    pub width: u8,
    /// Point inside the circuit this port is attached to.
    pub at: (i32, i32),
    /// Position on the instance (east-facing).
    pub offset: (i32, i32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentDef {
    /// Library name from the registry (e.g. "Gates"), or [`SUBCIRCUIT_LIBRARY`].
    pub library: String,
    /// Part name within the library (or the circuit name for subcircuits).
    pub part: String,
    pub at: (i32, i32),
    #[serde(default)]
    pub facing: Direction,
    #[serde(default)]
    pub options: OptionMap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireDef {
    pub a: (i32, i32),
    pub b: (i32, i32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Parse(String),
    Serialize(String),
    UnsupportedVersion(u32),
    UnknownLibrary(String),
    UnknownPart {
        library: String,
        part: String,
    },
    UnknownCircuit(String),
    /// Circuits instantiate each other in a loop (the chain, ending where it started).
    Cycle(Vec<String>),
    BadOptions {
        part: String,
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(m) => write!(f, "cannot parse .rcirc file: {m}"),
            Error::Serialize(m) => write!(f, "cannot write .rcirc file: {m}"),
            Error::UnsupportedVersion(v) => {
                write!(
                    f,
                    "file format version {v} is newer than supported version {FORMAT_VERSION}"
                )
            }
            Error::UnknownLibrary(l) => write!(f, "unknown library '{l}'"),
            Error::UnknownPart { library, part } => {
                write!(f, "library '{library}' has no part '{part}'")
            }
            Error::UnknownCircuit(c) => write!(f, "no circuit named '{c}'"),
            Error::Cycle(c) => write!(
                f,
                "circuits instantiate each other in a loop: {}",
                c.join(" -> ")
            ),
            Error::BadOptions { part, message } => write!(f, "bad options for '{part}': {message}"),
        }
    }
}

impl std::error::Error for Error {}

fn loc(p: (i32, i32)) -> Location {
    Location::new(p.0, p.1)
}

impl ComponentDef {
    /// A part with its default options, read from the registry.
    pub fn new(library: &str, part: &str, x: i32, y: i32) -> Result<Self, Error> {
        let comp = make_part(library, part)?;
        Ok(ComponentDef {
            library: library.into(),
            part: part.into(),
            at: (x, y),
            facing: Direction::East,
            options: comp.options(),
        })
    }
    pub fn facing(mut self, d: Direction) -> Self {
        self.facing = d;
        self
    }
    pub fn option(mut self, key: &str, v: impl Into<OptVal>) -> Self {
        self.options.set(key, v);
        self
    }
    /// Build the registry part with these options applied (not valid for subcircuits).
    pub fn build(&self) -> Result<Box<dyn Component>, Error> {
        let mut comp = make_part(&self.library, &self.part)?;
        comp.set_options(&self.options)
            .map_err(|message| Error::BadOptions {
                part: self.part.clone(),
                message,
            })?;
        Ok(comp)
    }
    /// Absolute location of the part's port `i` (registry parts only).
    pub fn port_location(&self, i: usize) -> Result<Location, Error> {
        let ports = self.build()?.ports();
        let off = self.facing.rotate(ports[i].offset);
        Ok(loc(self.at).translate(off.x, off.y))
    }
}

fn make_part(library: &str, part: &str) -> Result<Box<dyn Component>, Error> {
    let libs = all_libraries();
    let lib = libs
        .iter()
        .find(|l| l.name == library)
        .ok_or_else(|| Error::UnknownLibrary(library.into()))?;
    let desc = lib
        .parts
        .iter()
        .find(|p| p.name == part)
        .ok_or_else(|| Error::UnknownPart {
            library: library.into(),
            part: part.into(),
        })?;
    Ok((desc.make)())
}

impl ProjectFile {
    pub fn new(name: &str, main: &str) -> Self {
        ProjectFile {
            format_version: FORMAT_VERSION,
            metadata: Metadata {
                name: name.into(),
                description: String::new(),
                generator: "rcirc".into(),
            },
            circuits: vec![],
            main: main.into(),
        }
    }

    pub fn circuit(&self, name: &str) -> Option<&CircuitDef> {
        self.circuits.iter().find(|c| c.name == name)
    }

    /// Parse a `.rcirc` document.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let f: ProjectFile = ron::from_str(text).map_err(|e| Error::Parse(e.to_string()))?;
        if f.format_version > FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(f.format_version));
        }
        Ok(f)
    }

    /// Serialize to a pretty-printed `.rcirc` document.
    pub fn to_ron(&self) -> Result<String, Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| Error::Serialize(e.to_string()))
    }

    /// Build a fresh simulatable copy of the main circuit.
    pub fn instantiate_main(&self) -> Result<Circuit, Error> {
        self.instantiate(&self.main)
    }

    /// Build a fresh simulatable copy of circuit `name` (subcircuits get their own state).
    pub fn instantiate(&self, name: &str) -> Result<Circuit, Error> {
        self.build_circuit(name, &mut vec![])
    }

    fn build_circuit(&self, name: &str, stack: &mut Vec<String>) -> Result<Circuit, Error> {
        if stack.iter().any(|s| s == name) {
            let mut chain = stack.clone();
            chain.push(name.into());
            return Err(Error::Cycle(chain));
        }
        let def = self
            .circuit(name)
            .ok_or_else(|| Error::UnknownCircuit(name.into()))?;
        stack.push(name.into());
        let mut circuit = Circuit::new();
        for c in &def.components {
            let comp: Box<dyn Component> = if c.library == SUBCIRCUIT_LIBRARY {
                let inner_def = self
                    .circuit(&c.part)
                    .ok_or_else(|| Error::UnknownCircuit(c.part.clone()))?;
                let inner = self.build_circuit(&c.part, stack)?;
                let specs = inner_def
                    .interface
                    .iter()
                    .map(|i| SubPortSpec {
                        name: i.name.clone(),
                        input: i.input,
                        width: BitWidth::new(i.width),
                        at: loc(i.at),
                        offset: loc(i.offset),
                    })
                    .collect();
                Box::new(Subcircuit::new(&c.part, inner, specs))
            } else {
                c.build()?
            };
            circuit.add_component(loc(c.at), c.facing, comp);
        }
        for w in &def.wires {
            circuit.add_wire(loc(w.a), loc(w.b));
        }
        stack.pop();
        Ok(circuit)
    }
}

#[cfg(test)]
mod tests;
