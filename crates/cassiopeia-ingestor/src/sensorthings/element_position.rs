use derive_more::Display;
use serde_json::Value;

/// The position of an element of a `MultiDatastream`, counted from 1: the place of its value in the
/// `result` array of every Observation of the `MultiDatastream`, and of its `ObservedProperty`, unit
/// of measurement and observation type in the `MultiDatastream` (OGC 18-088 section 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Display)]
pub struct ElementPosition(usize);

impl ElementPosition {
    /// The position of the element at the zero-based `index`.
    #[must_use]
    pub const fn at_index(index: usize) -> ElementPosition {
        ElementPosition(index.saturating_add(1))
    }

    /// The position as the JSON number a reading's `multiDatastream` carries.
    #[must_use]
    pub fn json(self) -> Value {
        Value::from(self.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::element_position::ElementPosition;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn positions_count_from_one() {
        assert_eq!(ElementPosition::at_index(0).to_string(), "1");
        assert_eq!(ElementPosition::at_index(1).json(), json!(2));
        assert!(ElementPosition::at_index(0) < ElementPosition::at_index(1));
    }
}
