use cefact_units::UnitCode;

/// A memo of the UN/CEFACT unit codes a run's mappings declare.
///
/// A `unitCode` is a mapping literal, not record data: the whole run draws on the handful of codes
/// its mappings spell out, and resolving one costs a match against the entire UN/CEFACT code list.
/// The set is therefore bounded by the mappings rather than by the input, which is what makes a
/// cache that outlives a single entity safe, unlike an `observedAt`, which is unique per record and
/// would grow this vector without bound (see [`QualifierCache`](crate::qualifier_cache::QualifierCache)).
///
/// Lookup is a linear scan over that handful, cheaper than both the parse and a hash. The cache is
/// owned by whichever thread is transforming, one per rayon worker, so nothing is shared.
#[derive(Debug, Default)]
pub struct UnitCodeCache {
    codes: Vec<(Box<str>, Option<UnitCode>)>,
}

impl UnitCodeCache {
    /// Creates an empty cache.
    #[must_use]
    pub fn new() -> UnitCodeCache {
        UnitCodeCache::default()
    }

    /// The UN/CEFACT unit `code` denotes, parsed once per distinct code.
    ///
    /// A code the list does not contain caches as `None`, so an unrecognised unit is rejected once
    /// rather than re-matched on every attribute that declares it.
    pub fn unit_code(&mut self, code: &str) -> Option<UnitCode> {
        if let Some((_, parsed)) = self.codes.iter().find(|(known, _)| known.as_ref() == code) {
            return *parsed;
        }
        let parsed = code.parse().ok();
        self.codes.push((Box::from(code), parsed));
        parsed
    }
}

#[cfg(test)]
mod tests {
    use crate::unit_code_cache::UnitCodeCache;
    use cefact_units::UnitCode;

    #[test]
    fn two_distinct_unit_codes_both_resolve_through_one_cache() {
        let mut cache = UnitCodeCache::new();

        assert_eq!(cache.unit_code("KWH"), Some(UnitCode::Kwh));
        assert_eq!(cache.unit_code("VLT"), Some(UnitCode::Vlt));
        assert_eq!(cache.unit_code("KWH"), Some(UnitCode::Kwh));
        assert_eq!(cache.unit_code("VLT"), Some(UnitCode::Vlt));
    }

    #[test]
    fn an_unrecognised_unit_code_caches_as_none_and_stays_none() {
        let mut cache = UnitCodeCache::new();

        assert!(cache.unit_code("NOT-A-UNIT").is_none());
        assert!(cache.unit_code("NOT-A-UNIT").is_none());
        assert_eq!(cache.unit_code("CEL"), Some(UnitCode::Cel));
    }

    #[test]
    fn a_cache_reused_across_two_entities_resolves_each_entitys_codes() {
        let mut cache = UnitCodeCache::new();

        // The first entity's codes.
        assert_eq!(cache.unit_code("KWH"), Some(UnitCode::Kwh));
        assert_eq!(cache.unit_code("AMP"), Some(UnitCode::Amp));
        // The second entity declares different ones; the retained entries do not shadow them.
        assert_eq!(cache.unit_code("CEL"), Some(UnitCode::Cel));
        assert_eq!(cache.unit_code("VLT"), Some(UnitCode::Vlt));
        assert_eq!(cache.unit_code("KWH"), Some(UnitCode::Kwh));
    }
}
