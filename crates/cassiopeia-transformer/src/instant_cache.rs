use cassiopeia_ngsi_ld::value::types::Value;
use chrono::{DateTime, Utc};

/// A memo of the instant parses that repeat across one entity's attributes.
///
/// A mapping declares the same `observedAt` template on attribute after attribute, and often the
/// same `expiresAt` beside it, so the same text arrives once per attribute per record and each parse
/// is a full date-time scan, while an entity carries a handful of distinct texts at most. A linear
/// scan of that handful is cheaper than the parse and cheaper than hashing, so the cache is a vector
/// rather than a map.
///
/// The cache is strictly per entity: an instant qualifier is record data, so its distinct texts grow
/// with the input and a longer-lived linear-scan memo would turn quadratic. A `unitCode` is a mapping
/// literal instead, drawn from a fixed set, which is why it is memoised separately and for longer by
/// [`UnitCodeCache`](crate::unit_code_cache::UnitCodeCache).
#[derive(Debug, Default)]
pub struct InstantCache {
    parsed: Vec<(Box<str>, Option<DateTime<Utc>>)>,
}

impl InstantCache {
    /// Creates an empty cache, for one entity's worth of attributes.
    #[must_use]
    pub fn new() -> InstantCache {
        InstantCache::default()
    }

    /// The instant `value` denotes, text parsed once per distinct text.
    ///
    /// Only text goes through the cache. An instant the extraction stage already read is taken as it
    /// is, a number, read as a Unix epoch, is cheap to read directly, and any other shape carries no
    /// instant, so none of them is ever copied into it.
    pub fn instant(&mut self, value: &Value) -> Option<DateTime<Utc>> {
        let Some(text) = value.as_str() else {
            return value.try_parse_datetime();
        };
        if let Some((_, parsed)) = self.parsed.iter().find(|(known, _)| known.as_ref() == text) {
            return *parsed;
        }
        let parsed = value.try_parse_datetime();
        self.parsed.push((Box::from(text), parsed));
        parsed
    }
}

#[cfg(test)]
mod tests {
    use crate::instant_cache::InstantCache;
    use cassiopeia_ngsi_ld::value::types::{TemporalValue, Value};
    use chrono::{TimeZone, Utc};
    use pretty_assertions::{assert_eq, assert_ne};
    use serde_json::{Value as JsonValue, json};

    fn as_value(json: JsonValue) -> Value {
        Value::from(json)
    }

    #[test]
    fn a_repeated_timestamp_parses_to_the_same_instant_as_a_fresh_cache() {
        let value = as_value(json!("2026-04-03T22:00:20Z"));
        let mut cache = InstantCache::new();

        let first = cache.instant(&value);
        let cached = cache.instant(&value);
        let fresh = InstantCache::new().instant(&value);

        assert!(first.is_some());
        assert_eq!(first, cached);
        assert_eq!(first, fresh);
    }

    #[test]
    fn distinct_timestamp_texts_each_parse_to_their_own_instant() {
        let earlier = as_value(json!("2026-04-03T22:00:20Z"));
        let later = as_value(json!("2026-04-03T23:15:00Z"));
        let mut cache = InstantCache::new();

        let first = cache.instant(&earlier);
        let second = cache.instant(&later);

        assert_ne!(first, second);
        assert_eq!(cache.instant(&earlier), first);
        assert_eq!(cache.instant(&later), second);
    }

    #[test]
    fn a_numeric_epoch_timestamp_reads_as_the_same_instant_as_its_text() {
        let mut cache = InstantCache::new();

        let number = cache.instant(&as_value(json!(1_744_000_000)));

        assert!(number.is_some());
        assert_eq!(number, cache.instant(&as_value(json!("1744000000"))));
    }

    #[test]
    fn a_timestamp_that_is_neither_text_nor_an_epoch_yields_no_instant() {
        let mut cache = InstantCache::new();

        assert_eq!(cache.instant(&as_value(json!(2026))), None);
        assert_eq!(cache.instant(&as_value(json!(true))), None);
        assert_eq!(cache.instant(&as_value(json!(null))), None);
        assert!(cache.instant(&as_value(json!({"at": "2026-04-03T22:00:20Z"}))).is_none());
        assert!(cache.instant(&as_value(json!("not a timestamp"))).is_none());
    }

    #[test]
    fn an_instant_the_extraction_already_read_is_taken_as_it_is() {
        let mut cache = InstantCache::new();
        let instant = Utc.with_ymd_and_hms(2026, 4, 3, 22, 0, 20).unwrap();

        assert_eq!(cache.instant(&as_value(json!(null))), None);
        assert_eq!(cache.instant(&Value::Temporal(TemporalValue::DateTime(instant))), Some(instant));
    }
}
