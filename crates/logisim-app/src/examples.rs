//! Built-in example projects.

use crate::model::Editor;
use logisim_format::ComponentDef;

/// A half adder drawn with real wires: inputs A and B, outputs Sum (XOR) and Carry (AND).
pub fn half_adder() -> Editor {
    let mut e = Editor::new();
    e.project.metadata.name = "Half adder".into();
    e.project.metadata.description =
        "Sum = A xor B, Carry = A and B. Click an input to flip it.".into();
    let part =
        |lib: &str, name: &str, x, y| ComponentDef::new(lib, name, x, y).expect("registry part");
    e.place(
        part("Wiring", "Pin", 100, 100)
            .option("label", "A")
            .option("value", 1),
    );
    e.place(
        part("Wiring", "Pin", 100, 180)
            .option("label", "B")
            .option("value", 1),
    );
    e.place(part("Gates", "XOR Gate", 300, 120));
    e.place(part("Gates", "AND Gate", 300, 220));
    e.place(
        part("Wiring", "Pin", 400, 120)
            .option("label", "Sum")
            .option("output", true),
    );
    e.place(
        part("Wiring", "Pin", 400, 220)
            .option("label", "Carry")
            .option("output", true),
    );
    // A trunk at x=160, B trunk at x=200
    for (a, b) in [
        ((100, 100), (160, 100)),
        ((160, 100), (160, 200)),
        ((160, 100), (240, 100)),
        ((160, 200), (250, 200)),
        ((100, 180), (200, 180)),
        ((200, 140), (200, 240)),
        ((200, 140), (240, 140)),
        ((200, 240), (250, 240)),
        ((300, 120), (400, 120)),
        ((300, 220), (400, 220)),
    ] {
        e.add_wire_path(a, b);
    }
    e.selection.clear();
    e.mark_saved();
    e
}

/// A Counter and a 2-input AND gate, both left unconnected: shows how problems are flagged
/// (the counter's optional pins stay quiet; the AND gets one badge listing both inputs).
pub fn badge_demo() -> Editor {
    let mut e = Editor::new();
    e.project.metadata.name = "Problem badges demo".into();
    let part =
        |lib: &str, name: &str, x, y| ComponentDef::new(lib, name, x, y).expect("registry part");
    e.place(part("Memory", "Counter", 300, 140));
    e.place(part("Gates", "AND Gate", 120, 140));
    e.selection.clear();
    e.mark_saved();
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Sim;
    use logisim_core::{Location, Value};

    #[test]
    fn half_adder_example_computes() {
        let e = half_adder();
        let mut sim = Sim::build(&e.project, &e.current).unwrap();
        let (sum, carry) = (Location::new(400, 120), Location::new(400, 220));
        sim.snapshot([sum, carry]);
        assert_eq!(sim.value_at(sum), Some(Value::FALSE)); // 1 xor 1
        assert_eq!(sim.value_at(carry), Some(Value::TRUE)); // 1 and 1
        assert_eq!(e.circuit().interface.len(), 4);
        assert!(!e.dirty(), "a freshly loaded example is not modified");
    }
}
