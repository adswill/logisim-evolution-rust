//! Signal values and the simulation engine.

/// A single wire level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Low,
    High,
    Unknown,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_compare() {
        assert_eq!(Value::High, Value::High);
        assert_ne!(Value::Low, Value::High);
    }
}
