use derive_more::{Deref, Display, Into};

/// A record's `observedAt` temporal qualifier, as the exact text that flows into the temporal
/// entity's URN.
///
/// Kept as the source's own text rather than a parsed `DateTime` because it becomes a URN
/// r-component that must stay byte-identical to what the source declared: reformatting it through a
/// datetime type would change the identity of every temporal entity, so this newtype preserves the
/// value verbatim while still keeping it out of the bare-`String` domain.
/// The `Into` conversion hands the verbatim text over without copying it, so a store that persists
/// the qualifier as plain text moves it rather than re-allocating.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deref, Display, Into)]
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

    #[test]
    fn a_qualifier_preserves_its_exact_text() {
        assert_eq!(ObservedAt::new("2026-04-03T22:00:20Z").as_str(), "2026-04-03T22:00:20Z");
    }

    #[test]
    fn a_numeric_qualifier_is_preserved_as_written() {
        assert_eq!(ObservedAt::new("1775253620").to_string(), "1775253620");
    }
}
