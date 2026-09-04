use cassiopeia_ngsi_ld::value::convert::parse_datetime;
use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;

/// A memo of the `observedAt` parses that repeat across one entity's attributes.
///
/// A mapping declares the same `observedAt` template on attribute after attribute, so the same text
/// arrives once per attribute per record and each parse is a full date-time scan, while an entity
/// carries a handful of distinct texts at most. A linear scan of that handful is cheaper than the
/// parse and cheaper than hashing, so the cache is a vector rather than a map.
///
/// The cache is strictly per entity: an `observedAt` is record data, so its distinct texts grow with
/// the input and a longer-lived linear-scan memo would turn quadratic. A `unitCode` is a mapping
/// literal instead, drawn from a fixed set, which is why it is memoised separately and for longer by
/// [`UnitCodeCache`](crate::unit_code_cache::UnitCodeCache).
#[derive(Debug, Default)]
pub struct ObservedAtCache {
    parsed: Vec<(Box<str>, Option<DateTime<Utc>>)>,
}

impl ObservedAtCache {
    /// Creates an empty cache, for one entity's worth of attributes.
    #[must_use]
    pub fn new() -> ObservedAtCache {
        ObservedAtCache::default()
    }

    /// The instant `value` denotes, parsed once per distinct text.
    ///
    /// A non-string carries no instant and is rejected without touching the cache, so no arbitrary
    /// JSON is ever copied into it.
    pub fn observed_at(&mut self, value: &JsonValue) -> Option<DateTime<Utc>> {
        let text = value.as_str()?;
        if let Some((_, parsed)) = self.parsed.iter().find(|(known, _)| known.as_ref() == text) {
            return *parsed;
        }
        let parsed = parse_datetime(value);
        self.parsed.push((Box::from(text), parsed));
        parsed
    }
}

#[cfg(test)]
mod tests {
    use crate::observed_at_cache::ObservedAtCache;
    use serde_json::json;

    #[test]
    fn a_repeated_observed_at_parses_to_the_same_instant_as_a_fresh_cache() {
        let value = json!("2026-04-03T22:00:20Z");
        let mut cache = ObservedAtCache::new();

        let first = cache.observed_at(&value);
        let cached = cache.observed_at(&value);
        let fresh = ObservedAtCache::new().observed_at(&value);

        assert!(first.is_some());
        assert_eq!(first, cached);
        assert_eq!(first, fresh);
    }

    #[test]
    fn distinct_observed_at_texts_each_parse_to_their_own_instant() {
        let earlier = json!("2026-04-03T22:00:20Z");
        let later = json!("2026-04-03T23:15:00Z");
        let mut cache = ObservedAtCache::new();

        let first = cache.observed_at(&earlier);
        let second = cache.observed_at(&later);

        assert_ne!(first, second);
        assert_eq!(cache.observed_at(&earlier), first);
        assert_eq!(cache.observed_at(&later), second);
    }

    #[test]
    fn a_non_string_observed_at_yields_no_instant() {
        let mut cache = ObservedAtCache::new();

        assert!(cache.observed_at(&json!(1_744_000_000)).is_none());
        assert!(cache.observed_at(&json!(null)).is_none());
        assert!(cache.observed_at(&json!({"at": "2026-04-03T22:00:20Z"})).is_none());
        assert!(cache.observed_at(&json!("not a timestamp")).is_none());
    }
}
