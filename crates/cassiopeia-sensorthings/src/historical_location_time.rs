use crate::served_instant::served_instant;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

/// The member a served `HistoricalLocation` carries its `time` in (OGC 18-088 section 8.2.3): the time
/// the Thing was known at its Locations.
pub const TIME_MEMBER: &str = "time";

/// The instant the `time` of the served `HistoricalLocation` `served` names, as [`served_instant`]
/// reads it, or `None` when it carries none, or one that is no text or names no instant.
#[must_use]
pub fn historical_location_time(served: &Map<String, Value>) -> Option<DateTime<Utc>> {
    served.get(TIME_MEMBER)?.as_str().and_then(served_instant)
}

#[cfg(test)]
mod tests {
    use crate::{historical_location_time::historical_location_time, instant_text::instant_text};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn time_of(value: Value) -> Option<String> {
        historical_location_time(&Map::from_iter([("time".to_owned(), value)])).map(instant_text)
    }

    #[test]
    fn a_historical_location_s_time_is_read_from_its_member() {
        assert_eq!(time_of(json!("2026-11-12T12:04:00+02:00")).as_deref(), Some("2026-11-12T10:04:00Z"));
    }

    #[test]
    fn a_time_that_names_no_instant_or_no_time_is_none() {
        assert_eq!(time_of(json!("2026-11-12")), None);
        assert_eq!(time_of(json!(null)), None);
        assert_eq!(time_of(json!(1_762_941_840)), None);
        assert_eq!(historical_location_time(&Map::new()), None);
    }
}
