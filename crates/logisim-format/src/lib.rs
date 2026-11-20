//! `.rcirc` file format (RON-serialised).
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CircuitFile {
    pub name: String,
}

pub fn to_string(c: &CircuitFile) -> Result<String, ron::Error> {
    ron::to_string(c)
}
