//! Button: a 1-bit output that changes while pressed.
//!
//! Port order: 0 = output at (0,0). Active (default): low released, high pressed. Passive: inverse.
//! The UI sets the shared [`Button::handle`] and then calls `Circuit::refresh(id)`.

use std::cell::Cell;
use std::rc::Rc;

use logisim_core::{BitWidth, Component, OptionMap, OptionSpec, Port, PortKind, PropagateCtx, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonPress {
    /// Output is high while pressed (Logisim default).
    #[default]
    Active,
    /// Output is low while pressed.
    Passive,
}

const PRESS: [(&str, ButtonPress); 2] = [("active", ButtonPress::Active), ("passive", ButtonPress::Passive)];

/// True while the button is held down.
pub type ButtonHandle = Rc<Cell<bool>>;

pub struct Button {
    press: ButtonPress,
    pressed: ButtonHandle,
}

impl Button {
    pub fn new(press: ButtonPress) -> Self {
        Button { press, pressed: Rc::new(Cell::new(false)) }
    }
    pub fn handle(&self) -> ButtonHandle {
        Rc::clone(&self.pressed)
    }
}

impl Component for Button {
    fn name(&self) -> &str {
        "Button"
    }
    fn options(&self) -> OptionMap {
        OptionMap::new().with("press", OptionMap::name_of(self.press, &PRESS))
    }
    fn option_specs(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec::choice("press", "Pressed output", &["active", "passive"], "active"),
        ]
    }
    fn set_options(&mut self, m: &OptionMap) -> Result<(), String> {
        m.reject_unknown(&["press"])?;
        self.press = m.choice("press", &PRESS)?.unwrap_or(self.press);
        Ok(())
    }
    fn ports(&self) -> Vec<Port> {
        vec![Port::new(0, 0, PortKind::Output, BitWidth::ONE)]
    }
    fn propagate(&mut self, ctx: &mut dyn PropagateCtx) {
        let high = (self.press == ButtonPress::Active) == self.pressed.get();
        ctx.set_port(0, if high { Value::TRUE } else { Value::FALSE }, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::testutil::Rig;

    #[test]
    fn active_and_passive() {
        for (press, released, held) in [(ButtonPress::Active, Value::FALSE, Value::TRUE), (ButtonPress::Passive, Value::TRUE, Value::FALSE)] {
            let b = Button::new(press);
            let h = b.handle();
            let mut r = Rig::new(Box::new(b), &[]);
            assert_eq!(r.get(0), released);
            h.set(true);
            r.refresh();
            assert_eq!(r.get(0), held);
            h.set(false);
            r.refresh();
            assert_eq!(r.get(0), released);
        }
    }
}
