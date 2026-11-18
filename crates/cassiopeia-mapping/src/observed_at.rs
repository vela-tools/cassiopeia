use derive_more::{Deref, Display, Into};
use serde::{Deserialize, Serialize};

/// The text a record's `observedAt` rendered when it reads as no instant, as a report quotes it.
///
/// A record is ranked by the instant its `observedAt` names; text that names none cannot be ranked
/// by, and is kept only so the run can say which spelling it met. Kept as the source's own text
/// rather than reformatted, because the report names what the source wrote, while still keeping it
/// out of the bare-`String` domain. The `Into` conversion hands the text over without copying it. It
/// serializes as that bare text, so a disk-backed store persists it unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deref, Display, Into, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObservedAt(String);

impl ObservedAt {
    /// Wraps a temporal qualifier value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> ObservedAt {
        ObservedAt(value.into())
    }

    /// The qualifier as a string slice, for building the URN r-component.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::observed_at::ObservedAt;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_qualifier_preserves_its_exact_text() {
        assert_eq!(ObservedAt::new("2026-04-03T22:00:20Z").as_str(), "2026-04-03T22:00:20Z");
    }

    #[test]
    fn a_qualifier_serializes_as_its_bare_text() {
        let qualifier = ObservedAt::new("2026-04-03T22:00:20Z");

        assert_eq!(serde_json::to_value(&qualifier).unwrap(), serde_json::json!("2026-04-03T22:00:20Z"));
        assert_eq!(
            serde_json::from_value::<ObservedAt>(serde_json::json!("2026-04-03T22:00:20Z")).unwrap(),
            qualifier
        );
    }

    #[test]
    fn a_numeric_qualifier_is_preserved_as_written() {
        assert_eq!(ObservedAt::new("1775253620").to_string(), "1775253620");
    }
}
