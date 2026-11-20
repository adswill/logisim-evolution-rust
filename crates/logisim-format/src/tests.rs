use super::*;
use logisim_core::Value;

type P = (i32, i32);

/// Little builder: places parts and tunnels every listed port so no wire routing is needed.
struct Sheet {
    def: CircuitDef,
}

impl Sheet {
    fn new(name: &str) -> Self {
        Sheet {
            def: CircuitDef {
                name: name.into(),
                interface: vec![],
                components: vec![],
                wires: vec![],
            },
        }
    }
    fn tunnel(&mut self, at: P, name: &str) {
        let t = ComponentDef::new("Wiring", "Tunnel", at.0, at.1)
            .unwrap()
            .option("name", name);
        self.def.components.push(t);
    }
    /// Place a registry part; `tunnels` = (port index, tunnel name).
    fn part(&mut self, c: ComponentDef, tunnels: &[(usize, &str)]) {
        for (i, n) in tunnels {
            let l = c.port_location(*i).unwrap();
            self.tunnel((l.x, l.y), n);
        }
        self.def.components.push(c);
    }
    /// Place a subcircuit instance whose ports sit at the given offsets (east-facing).
    fn sub(&mut self, circuit: &str, at: P, ports: &[(P, &str)]) {
        self.def.components.push(ComponentDef {
            library: SUBCIRCUIT_LIBRARY.into(),
            part: circuit.into(),
            at,
            facing: Direction::East,
            mirror: false,
            options: OptionMap::new(),
        });
        for ((dx, dy), n) in ports {
            self.tunnel((at.0 + dx, at.1 + dy), n);
        }
    }
    fn iface(&mut self, name: &str, input: bool, at: P, offset: P) {
        self.def.interface.push(InterfaceDef {
            name: name.into(),
            input,
            width: 1,
            at,
            offset,
        });
        self.tunnel(at, name);
    }
}

fn gate(part: &str, x: i32, y: i32) -> ComponentDef {
    ComponentDef::new("Gates", part, x, y).unwrap()
}
fn pin(v: u64, x: i32, y: i32) -> ComponentDef {
    ComponentDef::new("Wiring", "Pin", x, y)
        .unwrap()
        .option("value", v as i64)
}

const HA_PORTS: [(P, &str); 4] = [
    ((-30, -10), "a"),
    ((-30, 10), "b"),
    ((30, -10), "s"),
    ((30, 10), "c"),
];

fn half_adder_def() -> CircuitDef {
    let mut s = Sheet::new("half_adder");
    s.iface("a", true, (0, 0), (-30, -10));
    s.iface("b", true, (0, 100), (-30, 10));
    s.iface("sum", false, (600, 0), (30, -10));
    s.iface("carry", false, (600, 100), (30, 10));
    s.part(gate("XOR Gate", 300, 0), &[(0, "sum"), (1, "a"), (2, "b")]);
    s.part(
        gate("AND Gate", 300, 100),
        &[(0, "carry"), (1, "a"), (2, "b")],
    );
    s.def
}

fn project_with(main: CircuitDef, others: Vec<CircuitDef>) -> ProjectFile {
    let mut p = ProjectFile::new("test", &main.name);
    p.circuits = others;
    p.circuits.push(main);
    p
}

/// Save to text, load again, and make sure nothing changed.
fn round_trip(p: &ProjectFile) -> ProjectFile {
    let text = p.to_ron().unwrap();
    let back = ProjectFile::parse(&text).unwrap();
    assert_eq!(&back, p, "round trip changed the project:\n{text}");
    back
}

fn read(c: &mut Circuit, at: P) -> Value {
    c.value_at(loc(at))
}

#[test]
fn half_adder_from_gates_saves_loads_and_simulates() {
    for (a, b) in [(0u64, 0u64), (0, 1), (1, 0), (1, 1)] {
        let mut s = Sheet::new("main");
        s.part(pin(a, 0, 0), &[(0, "a")]);
        s.part(pin(b, 0, 100), &[(0, "b")]);
        s.part(gate("XOR Gate", 300, 0), &[(0, "sum"), (1, "a"), (2, "b")]);
        s.part(
            gate("AND Gate", 300, 100),
            &[(0, "carry"), (1, "a"), (2, "b")],
        );
        s.tunnel((900, 0), "sum");
        s.tunnel((900, 100), "carry");
        let project = round_trip(&project_with(s.def, vec![]));
        let mut c = project.instantiate_main().unwrap();
        c.reset();
        assert!(!c.is_oscillating());
        assert_eq!(read(&mut c, (900, 0)).to_u64(), Some(a ^ b), "sum {a}+{b}");
        assert_eq!(
            read(&mut c, (900, 100)).to_u64(),
            Some(a & b),
            "carry {a}+{b}"
        );
    }
}

#[test]
fn subcircuit_half_adder() {
    for (a, b) in [(0u64, 0u64), (0, 1), (1, 0), (1, 1)] {
        let mut s = Sheet::new("main");
        s.part(pin(a, 0, 0), &[(0, "a")]);
        s.part(pin(b, 0, 100), &[(0, "b")]);
        s.sub("half_adder", (300, 0), &HA_PORTS);
        s.tunnel((900, 0), "s");
        s.tunnel((900, 100), "c");
        let project = round_trip(&project_with(s.def, vec![half_adder_def()]));
        let mut c = project.instantiate_main().unwrap();
        c.reset();
        assert_eq!(read(&mut c, (900, 0)).to_u64(), Some(a ^ b));
        assert_eq!(read(&mut c, (900, 100)).to_u64(), Some(a & b));
    }
}

#[test]
fn nested_subcircuits_full_adder_all_inputs() {
    // full_adder uses two half_adder instances and an OR gate; main uses full_adder.
    let mut fa = Sheet::new("full_adder");
    fa.iface("a", true, (0, 0), (-30, -20));
    fa.iface("b", true, (0, 100), (-30, 0));
    fa.iface("cin", true, (0, 200), (-30, 20));
    fa.iface("sum", false, (1500, 0), (30, -10));
    fa.iface("cout", false, (1500, 100), (30, 10));
    // HA1: a + b → s1, c1 ; HA2: s1 + cin → sum, c2 ; cout = c1 | c2
    let ha = |a: &'static str,
              b: &'static str,
              s: &'static str,
              c: &'static str|
     -> [(P, &'static str); 4] {
        [
            ((-30, -10), a),
            ((-30, 10), b),
            ((30, -10), s),
            ((30, 10), c),
        ]
    };
    fa.sub("half_adder", (300, 0), &ha("a", "b", "s1", "c1"));
    fa.sub("half_adder", (700, 0), &ha("s1", "cin", "sum", "c2"));
    fa.part(
        gate("OR Gate", 1000, 300),
        &[(0, "cout"), (1, "c1"), (2, "c2")],
    );
    let fa = fa.def;

    for bits in 0..8u64 {
        let (a, b, cin) = (bits & 1, bits >> 1 & 1, bits >> 2 & 1);
        let mut s = Sheet::new("main");
        s.part(pin(a, 0, 0), &[(0, "a")]);
        s.part(pin(b, 0, 100), &[(0, "b")]);
        s.part(pin(cin, 0, 200), &[(0, "cin")]);
        s.sub(
            "full_adder",
            (300, 0),
            &[
                ((-30, -20), "a"),
                ((-30, 0), "b"),
                ((-30, 20), "cin"),
                ((30, -10), "sum"),
                ((30, 10), "cout"),
            ],
        );
        s.tunnel((900, 0), "sum");
        s.tunnel((900, 100), "cout");
        let project = round_trip(&project_with(s.def, vec![half_adder_def(), fa.clone()]));
        let mut c = project.instantiate_main().unwrap();
        c.reset();
        assert!(!c.is_oscillating());
        let total = a + b + cin;
        assert_eq!(
            read(&mut c, (900, 0)).to_u64(),
            Some(total & 1),
            "sum of {a}+{b}+{cin}"
        );
        assert_eq!(
            read(&mut c, (900, 100)).to_u64(),
            Some(total >> 1),
            "carry of {a}+{b}+{cin}"
        );
    }
}

#[test]
fn each_instance_has_independent_state() {
    // two half adders in one main circuit with different inputs must not interfere
    let mut s = Sheet::new("main");
    s.part(pin(1, 0, 0), &[(0, "a1")]);
    s.part(pin(1, 0, 100), &[(0, "b1")]);
    s.part(pin(0, 0, 200), &[(0, "a2")]);
    s.part(pin(1, 0, 300), &[(0, "b2")]);
    let p = |a: &'static str, b: &'static str, s: &'static str, c: &'static str| {
        [
            ((-30, -10), a),
            ((-30, 10), b),
            ((30, -10), s),
            ((30, 10), c),
        ]
    };
    s.sub("half_adder", (300, 0), &p("a1", "b1", "s1", "c1"));
    s.sub("half_adder", (300, 400), &p("a2", "b2", "s2", "c2"));
    for (i, n) in ["s1", "c1", "s2", "c2"].iter().enumerate() {
        s.tunnel((900, 100 * i as i32), n);
    }
    let project = round_trip(&project_with(s.def, vec![half_adder_def()]));
    let mut c = project.instantiate_main().unwrap();
    c.reset();
    let got: Vec<_> = (0..4)
        .map(|i| read(&mut c, (900, 100 * i)).to_u64().unwrap())
        .collect();
    assert_eq!(got, vec![0, 1, 1, 0]); // 1+1 → s=0 c=1 ; 0+1 → s=1 c=0
}

#[test]
fn recursive_subcircuit_is_rejected() {
    let mut a = Sheet::new("a");
    a.sub("b", (0, 0), &[]);
    let mut b = Sheet::new("b");
    b.sub("a", (0, 0), &[]);
    let p = project_with(a.def, vec![b.def]);
    assert_eq!(
        p.instantiate_main().err(),
        Some(Error::Cycle(vec!["a".into(), "b".into(), "a".into()]))
    );
}

#[test]
fn errors_are_reported_clearly() {
    assert!(matches!(
        ProjectFile::parse("not ron"),
        Err(Error::Parse(_))
    ));
    let mut p = ProjectFile::new("x", "main");
    p.format_version = FORMAT_VERSION + 1;
    let text = p.to_ron().unwrap();
    assert_eq!(
        ProjectFile::parse(&text).err(),
        Some(Error::UnsupportedVersion(FORMAT_VERSION + 1))
    );
    assert!(matches!(
        ComponentDef::new("Nope", "AND Gate", 0, 0),
        Err(Error::UnknownLibrary(_))
    ));
    assert!(matches!(
        ComponentDef::new("Gates", "Nope", 0, 0),
        Err(Error::UnknownPart { .. })
    ));
    let mut s = Sheet::new("main");
    s.def
        .components
        .push(gate("AND Gate", 0, 0).option("inputs", 1));
    let err = project_with(s.def, vec![])
        .instantiate_main()
        .err()
        .unwrap();
    assert!(matches!(err, Error::BadOptions { .. }), "{err}");
    assert!(ProjectFile::new("x", "missing").instantiate_main().is_err());
}

#[test]
fn document_shape_is_stable() {
    let mut s = Sheet::new("main");
    s.def.components.push(
        gate("NAND Gate", 100, 50)
            .option("inputs", 3)
            .facing(Direction::North),
    );
    let text = project_with(s.def, vec![]).to_ron().unwrap();
    for needle in [
        "format_version: 1",
        "library: \"Gates\"",
        "part: \"NAND Gate\"",
        "facing: North",
        "\"inputs\": Int(3)",
    ] {
        assert!(text.contains(needle), "missing {needle} in:\n{text}");
    }
}

#[test]
fn mirror_round_trips_and_is_omitted_when_false() {
    let mut s = Sheet::new("main");
    s.def
        .components
        .push(gate("AND Gate", 100, 100).mirrored(true));
    s.def.components.push(gate("OR Gate", 300, 100));
    let project = project_with(s.def, vec![]);
    let text = project.to_ron().unwrap();
    assert_eq!(text.matches("mirror: true").count(), 1, "{text}");
    assert!(
        !text.contains("mirror: false"),
        "default is not written, so old readers and files are unaffected"
    );
    let back = ProjectFile::parse(&text).unwrap();
    assert_eq!(back, project);
    assert!(back.circuits[0].components[0].mirror);
    // a file written before the field existed still loads (mirror = false)
    let old = text.replace("mirror: true,", "");
    assert!(!ProjectFile::parse(&old).unwrap().circuits[0].components[0].mirror);
}

#[test]
fn mirrored_part_simulates_with_flipped_ports() {
    // AND gate with inputs at y +-20; mirroring swaps which input is on top
    let plain = gate("AND Gate", 100, 100);
    let flipped = gate("AND Gate", 100, 100).mirrored(true);
    let a = (
        plain.port_location(1).unwrap(),
        flipped.port_location(1).unwrap(),
    );
    assert_eq!(a.0.y, 80);
    assert_eq!(a.1.y, 120);
    // wire pins to the flipped gate through its own port locations and simulate
    let mut s = Sheet::new("main");
    s.part(pin(1, 0, 0), &[(0, "a")]);
    s.part(pin(1, 0, 100), &[(0, "b")]);
    s.part(
        gate("AND Gate", 300, 0).mirrored(true),
        &[(0, "out"), (1, "a"), (2, "b")],
    );
    s.tunnel((900, 0), "out");
    let project = project_with(s.def, vec![]);
    let mut c = project.instantiate_main().unwrap();
    c.reset();
    assert_eq!(read(&mut c, (900, 0)).to_u64(), Some(1));
}
