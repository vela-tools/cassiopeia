use crate::{observed_at_cache::ObservedAtCache, unit_code_cache::UnitCodeCache};
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;

/// The already-parsed qualifier texts available while one entity's attributes are built.
///
/// A mapping repeats the same `observedAt` template and the same `unitCode` literal on attribute
/// after attribute, so both arrive identical once per attribute per record and both cost a real parse:
/// a full date-time scan, and a match against the whole UN/CEFACT code list. The two are memoised
/// separately because they live for different spans: an `observedAt` is record data and its memo must
/// die with the entity, while a `unitCode` is drawn from the mapping's fixed set and its memo is worth
/// keeping for a whole batch. Borrowing both here is what lets attribute building take one cache
/// argument without collapsing that distinction.
pub struct QualifierCache<'a> {
    observed_at: &'a mut ObservedAtCache,
    unit_codes: &'a mut UnitCodeCache,
}

impl<'a> QualifierCache<'a> {
    /// Borrows the per-entity `observedAt` memo and the longer-lived unit-code memo for one entity.
    #[must_use]
    pub const fn new(observed_at: &'a mut ObservedAtCache, unit_codes: &'a mut UnitCodeCache) -> QualifierCache<'a> {
        QualifierCache { observed_at, unit_codes }
    }

    /// The instant `value` denotes, parsed once per distinct text within this entity.
    pub fn observed_at(&mut self, value: &JsonValue) -> Option<DateTime<Utc>> {
        self.observed_at.observed_at(value)
    }

    /// The UN/CEFACT unit `code` denotes, parsed once per distinct code within the borrowed memo.
    pub fn unit_code(&mut self, code: &str) -> Option<UnitCode> {
        self.unit_codes.unit_code(code)
    }
}

#[cfg(test)]
mod tests {
    use crate::{observed_at_cache::ObservedAtCache, qualifier_cache::QualifierCache, unit_code_cache::UnitCodeCache};
    use cefact_units::UnitCode;
    use serde_json::json;

    #[test]
    fn both_qualifiers_resolve_through_one_borrowed_pair() {
        let mut observed_at = ObservedAtCache::new();
        let mut unit_codes = UnitCodeCache::new();
        let mut cache = QualifierCache::new(&mut observed_at, &mut unit_codes);

        assert!(cache.observed_at(&json!("2026-04-03T22:00:20Z")).is_some());
        assert_eq!(cache.unit_code("KWH"), Some(UnitCode::Kwh));
    }

    #[test]
    fn a_second_entity_borrowing_the_same_unit_codes_keeps_its_own_observed_at_memo() {
        // The unit-code memo outlives the entity; the `observedAt` memo does not, which is the whole
        // reason the two are separate.
        let mut unit_codes = UnitCodeCache::new();

        let first = {
            let mut observed_at = ObservedAtCache::new();
            let mut cache = QualifierCache::new(&mut observed_at, &mut unit_codes);
            assert_eq!(cache.unit_code("CEL"), Some(UnitCode::Cel));
            cache.observed_at(&json!("2026-04-03T22:00:20Z"))
        };

        let mut observed_at = ObservedAtCache::new();
        let mut cache = QualifierCache::new(&mut observed_at, &mut unit_codes);
        let second = cache.observed_at(&json!("2026-04-03T23:15:00Z"));

        assert!(first.is_some());
        assert!(second.is_some());
        assert_ne!(first, second);
        assert_eq!(cache.unit_code("CEL"), Some(UnitCode::Cel));
    }
}
