use cassiopeia_mapping::observed_at::ObservedAt;
use serde::{Deserialize, Serialize};

/// What a record's `observedAt` template yielded, which a current-state store ranks the record by.
///
/// A template that fails is kept apart from a record that carries no timestamp: both rank the same,
/// below any declared timestamp, but a record the store discards for a failed template is a loss the
/// run has to report, while a record without a timestamp is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordTimestamp {
    /// The record declares this `observedAt`, verbatim.
    Declared(ObservedAt),
    /// The mapping is not temporal, or the record's timestamp field is absent or empty.
    Absent,
    /// The mapping's `observedAt` template could not be resolved for the record.
    Unresolvable,
}

impl RecordTimestamp {
    /// The text the record ranks by, `None` when it ranks below every declared timestamp.
    #[must_use]
    pub fn ranking_text(&self) -> Option<&str> {
        match self {
            RecordTimestamp::Declared(observed_at) => Some(observed_at.as_str()),
            RecordTimestamp::Absent | RecordTimestamp::Unresolvable => None,
        }
    }
}

impl From<Option<ObservedAt>> for RecordTimestamp {
    fn from(observed_at: Option<ObservedAt>) -> RecordTimestamp {
        observed_at.map_or(RecordTimestamp::Absent, RecordTimestamp::Declared)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_store::record_timestamp::RecordTimestamp;
    use cassiopeia_mapping::observed_at::ObservedAt;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_a_declared_timestamp_carries_ranking_text() {
        assert_eq!(
            RecordTimestamp::Declared(ObservedAt::new("2026-10-25T08:00:00Z")).ranking_text(),
            Some("2026-10-25T08:00:00Z")
        );
        assert_eq!(RecordTimestamp::Absent.ranking_text(), None);
        assert_eq!(RecordTimestamp::Unresolvable.ranking_text(), None);
    }

    #[test]
    fn a_present_timestamp_is_declared_and_a_missing_one_is_absent() {
        assert_eq!(
            RecordTimestamp::from(Some(ObservedAt::new("1775253620"))),
            RecordTimestamp::Declared(ObservedAt::new("1775253620"))
        );
        assert_eq!(RecordTimestamp::from(None), RecordTimestamp::Absent);
    }
}
