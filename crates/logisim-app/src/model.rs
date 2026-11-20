//! The editor document model: pure data + commands, no UI. Everything the editor can do to a
//! project goes through [`Command`]s so undo/redo is exact.

use logisim_core::{Direction, OptVal};
use logisim_format::{CircuitDef, ComponentDef, Error, InterfaceDef, ProjectFile, WireDef, SUBCIRCUIT_LIBRARY};
use std::path::PathBuf;

pub type P = (i32, i32);

/// One reversible edit of a circuit. `apply` returns the command that undoes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    InsertComponent { idx: usize, def: ComponentDef },
    RemoveComponent { idx: usize },
    ReplaceComponent { idx: usize, def: ComponentDef },
    InsertWire { idx: usize, wire: WireDef },
    RemoveWire { idx: usize },
    ReplaceWire { idx: usize, wire: WireDef },
    Batch(Vec<Command>),
}

impl Command {
    pub fn apply(&self, c: &mut CircuitDef) -> Command {
        match self {
            Command::InsertComponent { idx, def } => {
                c.components.insert(*idx, def.clone());
                Command::RemoveComponent { idx: *idx }
            }
            Command::RemoveComponent { idx } => {
                Command::InsertComponent { idx: *idx, def: c.components.remove(*idx) }
            }
            Command::ReplaceComponent { idx, def } => {
                let old = std::mem::replace(&mut c.components[*idx], def.clone());
                Command::ReplaceComponent { idx: *idx, def: old }
            }
            Command::InsertWire { idx, wire } => {
                c.wires.insert(*idx, *wire);
                Command::RemoveWire { idx: *idx }
            }
            Command::RemoveWire { idx } => Command::InsertWire { idx: *idx, wire: c.wires.remove(*idx) },
            Command::ReplaceWire { idx, wire } => {
                let old = std::mem::replace(&mut c.wires[*idx], *wire);
                Command::ReplaceWire { idx: *idx, wire: old }
            }
            Command::Batch(cmds) => {
                let mut inv: Vec<Command> = cmds.iter().map(|x| x.apply(c)).collect();
                inv.reverse();
                Command::Batch(inv)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sel {
    Comp(usize),
    Wire(usize),
}

/// A project being edited: the data, which circuit is open, selection and history.
pub struct Editor {
    pub project: ProjectFile,
    pub current: String,
    pub selection: Vec<Sel>,
    pub path: Option<PathBuf>,
    /// Bumped on every change to the project; views and simulations rebuild when it changes.
    pub version: u64,
    pub saved_version: u64,
    undo: Vec<(String, Command)>,
    redo: Vec<(String, Command)>,
}

fn empty_circuit(name: &str) -> CircuitDef {
    CircuitDef { name: name.into(), interface: vec![], components: vec![], wires: vec![] }
}

impl Editor {
    pub fn new() -> Self {
        let mut project = ProjectFile::new("Untitled", "main");
        project.circuits.push(empty_circuit("main"));
        Editor {
            project,
            current: "main".into(),
            selection: vec![],
            path: None,
            version: 0,
            saved_version: 0,
            undo: vec![],
            redo: vec![],
        }
    }

    pub fn from_project(project: ProjectFile, path: Option<PathBuf>) -> Self {
        let current = project.main.clone();
        Editor { project, current, path, ..Editor::new() }
    }

    pub fn dirty(&self) -> bool {
        self.version != self.saved_version
    }
    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }

    pub fn circuit(&self) -> &CircuitDef {
        self.project.circuit(&self.current).expect("current circuit exists")
    }
    fn circuit_mut(&mut self, name: &str) -> Option<&mut CircuitDef> {
        self.project.circuits.iter_mut().find(|c| c.name == name)
    }

    /// Run a command on the open circuit and record it for undo.
    pub fn exec(&mut self, cmd: Command) {
        let name = self.current.clone();
        let Some(c) = self.circuit_mut(&name) else { return };
        let inv = cmd.apply(c);
        c.interface = derive_interface(c);
        self.undo.push((name, inv));
        self.redo.clear();
        self.version += 1;
        self.selection.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some((name, inv)) = self.undo.pop() {
            if let Some(c) = self.circuit_mut(&name) {
                let redo = inv.apply(c);
                c.interface = derive_interface(c);
                self.redo.push((name.clone(), redo));
                self.current = name;
                self.version += 1;
                self.selection.clear();
            }
        }
    }
    pub fn redo(&mut self) {
        if let Some((name, cmd)) = self.redo.pop() {
            if let Some(c) = self.circuit_mut(&name) {
                let inv = cmd.apply(c);
                c.interface = derive_interface(c);
                self.undo.push((name.clone(), inv));
                self.current = name;
                self.version += 1;
                self.selection.clear();
            }
        }
    }

    // ---- high-level edits -----------------------------------------------------------

    pub fn place(&mut self, def: ComponentDef) {
        let idx = self.circuit().components.len();
        self.exec(Command::InsertComponent { idx, def });
        self.selection = vec![Sel::Comp(idx)];
    }

    /// Wires from `a` to `b`: one segment if aligned, otherwise horizontal then vertical.
    pub fn add_wire_path(&mut self, a: P, b: P) {
        if a == b {
            return;
        }
        let segs: Vec<WireDef> = if a.0 == b.0 || a.1 == b.1 {
            vec![WireDef { a, b }]
        } else {
            let corner = (b.0, a.1);
            vec![WireDef { a, b: corner }, WireDef { a: corner, b }]
        };
        let mut n = self.circuit().wires.len();
        let cmds = segs
            .into_iter()
            .filter(|w| !self.circuit().wires.contains(w))
            .map(|wire| {
                n += 1;
                Command::InsertWire { idx: n - 1, wire }
            })
            .collect::<Vec<_>>();
        if !cmds.is_empty() {
            self.exec(Command::Batch(cmds));
        }
    }

    pub fn delete_selection(&mut self) {
        let mut comps: Vec<usize> = vec![];
        let mut wires: Vec<usize> = vec![];
        for s in &self.selection {
            match s {
                Sel::Comp(i) => comps.push(*i),
                Sel::Wire(i) => wires.push(*i),
            }
        }
        // remove highest index first so earlier indexes stay valid
        comps.sort_unstable_by(|a, b| b.cmp(a));
        wires.sort_unstable_by(|a, b| b.cmp(a));
        let mut cmds: Vec<Command> = comps.into_iter().map(|idx| Command::RemoveComponent { idx }).collect();
        cmds.extend(wires.into_iter().map(|idx| Command::RemoveWire { idx }));
        if !cmds.is_empty() {
            self.exec(Command::Batch(cmds));
        }
    }

    pub fn move_selection(&mut self, dx: i32, dy: i32) {
        if (dx, dy) == (0, 0) {
            return;
        }
        let sel = self.selection.clone();
        let c = self.circuit().clone();
        let cmds: Vec<Command> = sel
            .iter()
            .map(|s| match s {
                Sel::Comp(i) => {
                    let mut def = c.components[*i].clone();
                    def.at = (def.at.0 + dx, def.at.1 + dy);
                    Command::ReplaceComponent { idx: *i, def }
                }
                Sel::Wire(i) => {
                    let w = c.wires[*i];
                    let wire = WireDef { a: (w.a.0 + dx, w.a.1 + dy), b: (w.b.0 + dx, w.b.1 + dy) };
                    Command::ReplaceWire { idx: *i, wire }
                }
            })
            .collect();
        self.exec(Command::Batch(cmds));
        self.selection = sel;
    }

    /// Rotate selected parts a quarter turn counter-clockwise.
    pub fn rotate_selection(&mut self) {
        let sel = self.selection.clone();
        let c = self.circuit().clone();
        let cmds: Vec<Command> = sel
            .iter()
            .filter_map(|s| match s {
                Sel::Comp(i) => {
                    let mut def = c.components[*i].clone();
                    def.facing = rotate_ccw(def.facing);
                    Some(Command::ReplaceComponent { idx: *i, def })
                }
                Sel::Wire(_) => None,
            })
            .collect();
        if !cmds.is_empty() {
            self.exec(Command::Batch(cmds));
            self.selection = sel;
        }
    }

    pub fn replace_component(&mut self, idx: usize, def: ComponentDef) {
        if self.circuit().components[idx] != def {
            self.exec(Command::ReplaceComponent { idx, def });
            self.selection = vec![Sel::Comp(idx)];
        }
    }

    /// Set one option of a component (validated by building the part first).
    pub fn set_option(&mut self, idx: usize, key: &str, v: impl Into<OptVal>) -> Result<(), Error> {
        let mut def = self.circuit().components[idx].clone();
        def.options.set(key, v);
        if def.library != SUBCIRCUIT_LIBRARY {
            def.build()?;
        }
        self.replace_component(idx, def);
        Ok(())
    }

    // ---- circuits ----------------------------------------------------------------------

    /// Add an empty circuit; returns its (possibly de-duplicated) name.
    pub fn add_circuit(&mut self, wanted: &str) -> String {
        let mut name = wanted.trim().to_string();
        if name.is_empty() {
            name = "circuit".into();
        }
        let mut n = 1;
        let base = name.clone();
        while self.project.circuit(&name).is_some() {
            n += 1;
            name = format!("{base}{n}");
        }
        self.project.circuits.push(empty_circuit(&name));
        self.version += 1;
        name
    }

    pub fn open_circuit(&mut self, name: &str) {
        if self.project.circuit(name).is_some() {
            self.current = name.into();
            self.selection.clear();
        }
    }

    /// Circuits that (directly) contain an instance of `name`.
    pub fn users_of(&self, name: &str) -> Vec<String> {
        self.project
            .circuits
            .iter()
            .filter(|c| c.components.iter().any(|d| d.library == SUBCIRCUIT_LIBRARY && d.part == name))
            .map(|c| c.name.clone())
            .collect()
    }

    /// Delete a circuit unless it is the main circuit or still used. Clears history (indexes shift).
    pub fn delete_circuit(&mut self, name: &str) -> Result<(), String> {
        if self.project.circuits.len() <= 1 || name == self.project.main {
            return Err("cannot delete the main circuit".into());
        }
        let users = self.users_of(name);
        if !users.is_empty() {
            return Err(format!("'{name}' is used by {}", users.join(", ")));
        }
        self.project.circuits.retain(|c| c.name != name);
        self.undo.retain(|(n, _)| n != name);
        self.redo.retain(|(n, _)| n != name);
        if self.current == name {
            self.current = self.project.main.clone();
        }
        self.version += 1;
        Ok(())
    }

    pub fn set_main(&mut self, name: &str) {
        if self.project.circuit(name).is_some() {
            self.project.main = name.into();
            self.version += 1;
        }
    }

    /// A subcircuit instance of `name`, ready to place (refuses recursion into the open circuit).
    pub fn subcircuit_def(&self, name: &str, at: P) -> Result<ComponentDef, String> {
        if name == self.current || self.uses_transitively(name, &self.current) {
            return Err(format!("'{name}' contains '{}', placing it would make a loop", self.current));
        }
        Ok(ComponentDef {
            library: SUBCIRCUIT_LIBRARY.into(),
            part: name.into(),
            at,
            facing: Direction::East,
            options: Default::default(),
        })
    }

    fn uses_transitively(&self, outer: &str, inner: &str) -> bool {
        let Some(c) = self.project.circuit(outer) else { return false };
        c.components
            .iter()
            .filter(|d| d.library == SUBCIRCUIT_LIBRARY)
            .any(|d| d.part == inner || self.uses_transitively(&d.part, inner))
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

pub fn rotate_ccw(d: Direction) -> Direction {
    match d {
        Direction::East => Direction::North,
        Direction::North => Direction::West,
        Direction::West => Direction::South,
        Direction::South => Direction::East,
    }
}

/// Subcircuit interface from the labelled Pins of a circuit (Logisim's behaviour): input pins
/// on the left, output pins on the right, each side ordered top to bottom.
pub fn derive_interface(c: &CircuitDef) -> Vec<InterfaceDef> {
    let mut pins: Vec<(&ComponentDef, bool, String, u8)> = vec![];
    for d in c.components.iter().filter(|d| d.library == "Wiring" && d.part == "Pin") {
        let label = match d.options.string("label") {
            Ok(Some(l)) if !l.trim().is_empty() => l.trim().to_string(),
            _ => continue,
        };
        let output = d.options.boolean("output").ok().flatten().unwrap_or(false);
        let width = d.options.int("width", 1, 64).ok().flatten().unwrap_or(1) as u8;
        pins.push((d, output, label, width));
    }
    pins.sort_by_key(|(d, _, _, _)| (d.at.1, d.at.0));
    let mut out = vec![];
    for want_output in [false, true] {
        for (i, (d, _, label, width)) in pins.iter().filter(|p| p.1 == want_output).enumerate() {
            out.push(InterfaceDef {
                name: label.clone(),
                input: !want_output,
                width: *width,
                at: d.at,
                offset: (if want_output { 60 } else { 0 }, 20 * i as i32),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(x: i32, y: i32) -> ComponentDef {
        ComponentDef::new("Gates", "AND Gate", x, y).unwrap()
    }

    #[test]
    fn undo_redo_restores_exactly() {
        let mut e = Editor::new();
        let before = e.project.clone();
        e.place(gate(100, 100));
        e.place(gate(200, 100));
        e.add_wire_path((0, 0), (50, 30));
        assert_eq!(e.circuit().components.len(), 2);
        assert_eq!(e.circuit().wires.len(), 2); // L-shaped: two segments
        let after = e.project.clone();
        for _ in 0..3 {
            e.undo();
        }
        assert_eq!(e.project, before);
        assert!(!e.can_undo() && e.can_redo());
        for _ in 0..3 {
            e.redo();
        }
        assert_eq!(e.project, after);
    }

    #[test]
    fn delete_move_rotate_are_undoable() {
        let mut e = Editor::new();
        e.place(gate(0, 0));
        e.place(gate(100, 0));
        e.add_wire_path((0, 0), (40, 0));
        let base = e.project.clone();
        e.selection = vec![Sel::Comp(0), Sel::Wire(0)];
        e.move_selection(20, 10);
        assert_eq!(e.circuit().components[0].at, (20, 10));
        assert_eq!(e.circuit().wires[0].a, (20, 10));
        assert_eq!(e.selection.len(), 2);
        e.rotate_selection();
        assert_eq!(e.circuit().components[0].facing, Direction::North);
        e.selection = vec![Sel::Comp(0), Sel::Comp(1), Sel::Wire(0)];
        e.delete_selection();
        assert!(e.circuit().components.is_empty() && e.circuit().wires.is_empty());
        e.undo();
        e.undo();
        e.undo();
        assert_eq!(e.project, base);
    }

    #[test]
    fn set_option_validates() {
        let mut e = Editor::new();
        e.place(gate(0, 0));
        assert!(e.set_option(0, "inputs", 1).is_err());
        assert!(e.set_option(0, "inputs", 4).is_ok());
        assert_eq!(e.circuit().components[0].options.int("inputs", 0, 99), Ok(Some(4)));
        e.undo();
        assert_eq!(e.circuit().components[0].options.int("inputs", 0, 99), Ok(Some(2)));
    }

    #[test]
    fn duplicate_wire_is_ignored_and_dirty_tracks() {
        let mut e = Editor::new();
        assert!(!e.dirty());
        e.add_wire_path((0, 0), (10, 0));
        let v = e.version;
        e.add_wire_path((0, 0), (10, 0));
        assert_eq!(e.version, v);
        assert!(e.dirty());
        e.mark_saved();
        assert!(!e.dirty());
    }

    #[test]
    fn interface_follows_labelled_pins() {
        let mut e = Editor::new();
        let pin = |x, y| ComponentDef::new("Wiring", "Pin", x, y).unwrap();
        e.place(pin(0, 0).option("label", "a"));
        e.place(pin(0, 40).option("label", "b"));
        e.place(pin(300, 0).option("label", "q").option("output", true));
        e.place(pin(0, 90)); // no label → not in the interface
        let i = &e.circuit().interface;
        assert_eq!(i.iter().map(|p| (p.name.as_str(), p.input)).collect::<Vec<_>>(), [("a", true), ("b", true), ("q", false)]);
        assert_eq!(i[1].offset, (0, 20));
        assert_eq!(i[2].offset, (60, 0));
        e.undo();
        e.undo();
        e.undo();
        assert_eq!(e.circuit().interface.len(), 2);
    }

    #[test]
    fn circuits_and_loop_protection() {
        let mut e = Editor::new();
        let sub = e.add_circuit("sub");
        assert_eq!(e.add_circuit("sub"), "sub2");
        e.open_circuit(&sub);
        // sub can't contain itself or main (once main contains sub)
        assert!(e.subcircuit_def("sub", (0, 0)).is_err());
        e.open_circuit("main");
        let def = e.subcircuit_def("sub", (0, 0)).unwrap();
        e.place(def);
        assert_eq!(e.users_of("sub"), vec!["main"]);
        e.open_circuit("sub");
        assert!(e.subcircuit_def("main", (0, 0)).is_err());
        assert!(e.delete_circuit("sub").is_err()); // still used
        assert!(e.delete_circuit("main").is_err());
        assert!(e.delete_circuit("sub2").is_ok());
    }
}
