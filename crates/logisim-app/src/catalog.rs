//! The parts catalog the editor shows: every registry part with a one-line description.

use logisim_components::all_libraries;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPart {
    pub library: String,
    pub name: String,
    pub description: String,
}

impl CatalogPart {
    /// Stable id used for favourites and recents: `Library/Part`.
    pub fn id(&self) -> String {
        format!("{}/{}", self.library, self.name)
    }
}

/// One-line help for a part. Parts without an entry get a generic line, so a new library works
/// without touching this file (add entries to make it nicer).
pub fn describe(library: &str, part: &str) -> String {
    let d = match (library, part) {
        ("Gates", "NOT Gate") => "Inverts its input: 1 becomes 0 and 0 becomes 1.",
        ("Gates", "Buffer") => "Passes its input through unchanged (restores a clean signal).",
        ("Gates", "AND Gate") => "Output is 1 only when every input is 1.",
        ("Gates", "OR Gate") => "Output is 1 when at least one input is 1.",
        ("Gates", "NAND Gate") => "AND followed by NOT: output is 0 only when every input is 1.",
        ("Gates", "NOR Gate") => "OR followed by NOT: output is 1 only when every input is 0.",
        ("Gates", "XOR Gate") => "Output is 1 when an odd number of inputs are 1.",
        ("Gates", "XNOR Gate") => "XOR followed by NOT: output is 1 when an even number of inputs are 1.",
        ("Gates", "Controlled Buffer") => "Tri-state buffer: passes the input only while its enable input is 1, otherwise floats.",
        ("Gates", "Controlled Inverter") => "Tri-state inverter: inverts the input while enabled, otherwise floats.",
        ("Gates", "Odd Parity") => "Output is 1 when an odd number of inputs are 1 (an odd-parity check).",
        ("Gates", "Even Parity") => "Output is 1 when an even number of inputs are 1 (an even-parity check).",
        ("Gates", "PLA") => "Programmable logic array: outputs are defined by a table of AND terms combined with OR.",
        ("Wiring", "Do Not Connect") => "Marks a connection point that is intentionally left unconnected.",
        ("Wiring", "Power On Reset") => "Outputs one level when the simulation starts, then the opposite after a short delay (to reset circuits).",
        ("Wiring", "Splitter") => "Splits a multi-bit bus into smaller groups of bits, or joins them back.",
        ("Wiring", "Pin") => "An input you can flip while simulating, or an output that shows a value. Give it a label to make it a port.",
        ("Wiring", "Probe") => "Shows the value on a wire without driving it.",
        ("Wiring", "Tunnel") => "Connects every tunnel with the same name without drawing a wire between them.",
        ("Wiring", "Pull Resistor") => "Gives a floating wire a default value (pull up to 1, pull down to 0).",
        ("Wiring", "Clock") => "A square wave that toggles every tick; use Run or Step to drive it.",
        ("Wiring", "Constant") => "Drives a fixed value onto a wire.",
        ("Wiring", "Power") => "Drives all-ones (logic 1) onto a wire.",
        ("Wiring", "Ground") => "Drives all-zeros (logic 0) onto a wire.",
        ("Wiring", "Transistor") => "A switch controlled by its gate: P conducts on 0, N conducts on 1.",
        ("Wiring", "Transmission Gate") => "Passes its input when the two gate inputs are opposite (0 and 1).",
        ("Wiring", "Bit Extender") => "Widens a value by repeating zero, one, the sign bit or an input bit.",
        _ => return format!("{part} from the {library} library."),
    };
    d.to_string()
}

/// Every placeable part of every registered library, in menu order.
pub fn all_parts() -> Vec<CatalogPart> {
    all_libraries()
        .into_iter()
        .flat_map(|lib| {
            lib.parts
                .iter()
                .map(|p| CatalogPart {
                    library: lib.name.to_string(),
                    name: p.name.to_string(),
                    description: describe(lib.name, p.name),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_part_is_listed_with_a_description() {
        let parts = all_parts();
        assert!(parts.iter().any(|p| p.id() == "Gates/AND Gate"));
        assert!(parts.iter().all(|p| !p.description.is_empty()));
        let total: usize = all_libraries().iter().map(|l| l.parts.len()).sum();
        assert_eq!(parts.len(), total);
    }

    #[test]
    fn core_parts_have_real_descriptions() {
        // parts other libraries add later fall back to a generic line on purpose
        for (lib, part) in [
            ("Gates", "AND Gate"),
            ("Gates", "XOR Gate"),
            ("Gates", "Odd Parity"),
            ("Wiring", "Pin"),
            ("Wiring", "Splitter"),
            ("Wiring", "Tunnel"),
            ("Wiring", "Clock"),
        ] {
            assert!(
                !describe(lib, part).ends_with("library."),
                "{lib}/{part} has only the generic description"
            );
        }
    }
}
