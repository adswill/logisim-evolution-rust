//! Live simulation of the open circuit.

use logisim_core::{Circuit, CompId, Location, Value};
use logisim_format::{CircuitDef, Error, ProjectFile};
use std::collections::HashMap;

pub struct Sim {
    pub circuit: Circuit,
    pub ticks: u64,
    values: HashMap<Location, Value>,
}

impl Sim {
    /// Build and settle a fresh simulation of circuit `name`.
    pub fn build(project: &ProjectFile, name: &str) -> Result<Sim, Error> {
        let mut circuit = project.instantiate(name)?;
        circuit.reset();
        Ok(Sim {
            circuit,
            ticks: 0,
            values: HashMap::new(),
        })
    }

    pub fn tick(&mut self) {
        self.circuit.tick();
        self.ticks += 1;
    }

    pub fn oscillating(&self) -> bool {
        self.circuit.is_oscillating()
    }

    /// Change a Pin's value in the live simulation (component index = position in the def).
    pub fn set_pin(&mut self, idx: usize, options: &logisim_core::OptionMap) {
        let id = CompId(idx);
        if self.circuit.component_mut(id).set_options(options).is_ok() {
            self.circuit.refresh(id);
        }
    }

    /// Record the net value at every point the painter will ask about.
    pub fn snapshot(&mut self, points: impl IntoIterator<Item = Location>) {
        self.values.clear();
        for p in points {
            let v = self.circuit.value_at(p);
            self.values.insert(p, v);
        }
    }

    pub fn value_at(&self, p: Location) -> Option<Value> {
        self.values.get(&p).copied().filter(|v| *v != Value::NIL)
    }
}

/// All points of interest of a circuit definition: wire ends only (ports are added by the caller).
pub fn wire_points(c: &CircuitDef) -> impl Iterator<Item = Location> + '_ {
    c.wires
        .iter()
        .flat_map(|w| [Location::new(w.a.0, w.a.1), Location::new(w.b.0, w.b.1)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_format::ComponentDef;

    #[test]
    fn poking_a_pin_updates_the_net() {
        let mut p = ProjectFile::new("t", "main");
        let pin = ComponentDef::new("Wiring", "Pin", 0, 0)
            .unwrap()
            .option("value", 0);
        let tun = ComponentDef::new("Wiring", "Tunnel", 0, 0)
            .unwrap()
            .option("name", "x");
        p.circuits.push(CircuitDef {
            name: "main".into(),
            interface: vec![],
            components: vec![pin.clone(), tun],
            wires: vec![],
        });
        let mut sim = Sim::build(&p, "main").unwrap();
        let at = Location::new(0, 0);
        sim.snapshot([at]);
        assert_eq!(sim.value_at(at), Some(Value::FALSE));
        sim.set_pin(0, &pin.options.clone().with("value", 1));
        sim.snapshot([at]);
        assert_eq!(sim.value_at(at), Some(Value::TRUE));
        assert!(!sim.oscillating());
    }
}
