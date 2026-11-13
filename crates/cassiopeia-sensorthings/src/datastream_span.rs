use crate::phenomenon_time::PhenomenonTime;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

/// Whether the served Datastream `datastream` states a `phenomenonTime` that ends before `instant`.
///
/// A Datastream's `phenomenonTime` spans the `phenomenonTime`s of all its Observations (OGC 18-088
/// section 8.2.4, Table 10), so one that ends before `instant` holds no reading from `instant` on,
/// and a read that starts there need not request it. One without a readable `phenomenonTime` may
/// hold any reading.
#[must_use]
pub fn ended_before(datastream: &Map<String, Value>, instant: DateTime<Utc>) -> bool {
    PhenomenonTime::of(datastream).is_some_and(|time| time.instant() < instant)
}

#[cfg(test)]
mod tests {
    use crate::datastream_span::ended_before;
    use chrono::{DateTime, Utc};
    use serde_json::{Map, Value, json};

    fn datastream(phenomenon_time: Value) -> Map<String, Value> {
        Map::from_iter([("phenomenonTime".to_owned(), phenomenon_time)])
    }

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    #[test]
    fn a_span_that_ends_before_the_instant_has_ended() {
        let span = datastream(json!("2026-01-01T00:00:00Z/2026-11-11T09:59:59Z"));

        assert!(ended_before(&span, instant("2026-11-11T10:00:00Z")));
    }

    #[test]
    fn a_span_that_ends_at_or_after_the_instant_has_not_ended() {
        for end in ["2026-11-11T10:00:00Z", "2026-11-12T00:00:00Z"] {
            let span = datastream(json!(format!("2026-01-01T00:00:00Z/{end}")));

            assert!(!ended_before(&span, instant("2026-11-11T10:00:00Z")), "{end}");
        }
    }

    #[test]
    fn a_datastream_without_a_readable_span_has_not_ended() {
        for value in [json!(null), json!("2026-01-01/2026-01-02"), json!(7)] {
            assert!(!ended_before(&datastream(value.clone()), instant("2026-11-11T10:00:00Z")), "{value}");
        }
        assert!(!ended_before(&Map::new(), instant("2026-11-11T10:00:00Z")));
    }
}
