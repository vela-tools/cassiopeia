/// Whether a temporal record with `new` `observedAt` supersedes the retained one at `existing`.
///
/// A current-state store keeps, per mapping, the record with the greatest record-level `observedAt`.
/// An absent timestamp ranks below any present one (`None < Some`), and a tie keeps the later-arriving
/// record. Timestamps are compared as verbatim text, monotonic under string compare for a fixed format
/// (RFC-3339, or fixed-width epoch); this is the record-level latest the store ranks by.
pub(crate) fn supersedes(new: Option<&str>, existing: Option<&str>) -> bool {
    new >= existing
}

#[cfg(test)]
mod tests {
    use crate::entity_store::supersession::supersedes;

    #[test]
    fn a_later_timestamp_supersedes_an_earlier_one() {
        assert!(supersedes(Some("2026-04-03T22:05:20Z"), Some("2026-04-03T22:00:20Z")));
        assert!(!supersedes(Some("2026-04-03T22:00:20Z"), Some("2026-04-03T22:05:20Z")));
    }

    #[test]
    fn a_present_timestamp_supersedes_an_absent_one_and_a_tie_keeps_the_later_arrival() {
        assert!(supersedes(Some("2026-04-03T22:00:20Z"), None));
        assert!(!supersedes(None, Some("2026-04-03T22:00:20Z")));
        assert!(supersedes(Some("2026-04-03T22:00:20Z"), Some("2026-04-03T22:00:20Z")));
    }
}
