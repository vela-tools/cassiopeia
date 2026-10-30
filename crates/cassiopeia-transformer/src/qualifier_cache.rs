use crate::{instant_cache::InstantCache, unit_code_cache::UnitCodeCache};
use cassiopeia_ngsi_ld::value::types::Value;
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};

/// The already-parsed qualifier texts available while one entity's attributes are built.
///
/// A mapping repeats the same `observedAt` template and the same `unitCode` literal on attribute
/// after attribute, and often the same `expiresAt` beside them, so they arrive identical once per
/// attribute per record and each costs a real parse: a full date-time scan, and a match against the
/// whole UN/CEFACT code list. The two are memoised separately because they live for different spans:
/// an instant qualifier is record data and its memo must die with the entity, while a `unitCode` is
/// drawn from the mapping's fixed set and its memo is worth keeping for a whole batch. Borrowing both
/// here is what lets attribute building take one cache argument without collapsing that distinction.
pub struct QualifierCache<'a> {
    instants: &'a mut InstantCache,
    unit_codes: &'a mut UnitCodeCache,
}

impl<'a> QualifierCache<'a> {
    /// Borrows the per-entity instant memo and the longer-lived unit-code memo for one entity.
    #[must_use]
    pub const fn new(instants: &'a mut InstantCache, unit_codes: &'a mut UnitCodeCache) -> QualifierCache<'a> {
        QualifierCache { instants, unit_codes }
    }

    /// The instant an `observedAt` or `expiresAt` `value` denotes, parsed once per distinct text
    /// within this entity.
    pub fn instant(&mut self, value: &Value) -> Option<DateTime<Utc>> {
        self.instants.instant(value)
    }

    /// The UN/CEFACT unit `code` denotes, parsed once per distinct code within the borrowed memo.
    pub fn unit_code(&mut self, code: &str) -> Option<UnitCode> {
        self.unit_codes.unit_code(code)
    }
}

#[cfg(test)]
mod tests {
    use crate::{instant_cache::InstantCache, qualifier_cache::QualifierCache, unit_code_cache::UnitCodeCache};
    use cassiopeia_ngsi_ld::value::types::Value;
    use cefact_units::UnitCode;
    use pretty_assertions::{assert_eq, assert_ne};

    #[test]
    fn an_instant_and_a_unit_code_resolve_through_one_borrowed_pair() {
        let mut instants = InstantCache::new();
        let mut unit_codes = UnitCodeCache::new();
        let mut cache = QualifierCache::new(&mut instants, &mut unit_codes);

        assert!(cache.instant(&Value::String("2026-04-03T22:00:20Z".into())).is_some());
        assert_eq!(cache.unit_code("KWH"), Some(UnitCode::Kwh));
    }

    #[test]
    fn a_second_entity_borrowing_the_same_unit_codes_keeps_its_own_instant_memo() {
        // The unit-code memo outlives the entity; the instant memo does not, which is the whole
        // reason the two are separate.
        let mut unit_codes = UnitCodeCache::new();

        let first = {
            let mut instants = InstantCache::new();
            let mut cache = QualifierCache::new(&mut instants, &mut unit_codes);
            assert_eq!(cache.unit_code("CEL"), Some(UnitCode::Cel));
            cache.instant(&Value::String("2026-04-03T22:00:20Z".into()))
        };

        let mut instants = InstantCache::new();
        let mut cache = QualifierCache::new(&mut instants, &mut unit_codes);
        let second = cache.instant(&Value::String("2026-04-03T23:15:00Z".into()));

        assert!(first.is_some());
        assert!(second.is_some());
        assert_ne!(first, second);
        assert_eq!(cache.unit_code("CEL"), Some(UnitCode::Cel));
    }
}
