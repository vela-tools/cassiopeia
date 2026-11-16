use chrono::{DateTime, SubsecRound, Utc};

/// How many fractional digits of a second an instant keeps: NGSI-LD times are precise to the
/// microsecond (ETSI GS CIM 009 v1.9.1 clause 4.6.3).
const FRACTION_DIGITS: u16 = 6;

/// The instant a served time names, such as an Observation's `phenomenonTime` (OGC 18-088 section
/// 8.2.7) or a `HistoricalLocation`'s `time` (section 8.2.3): an RFC 3339 date and time with a UTC
/// offset, read in UTC and truncated to microseconds, the precision NGSI-LD keeps.
///
/// A text without a time or without an offset names no instant, and an ISO 8601 duration is no
/// instant either.
#[must_use]
pub fn served_instant(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|instant| instant.with_timezone(&Utc).trunc_subsecs(FRACTION_DIGITS))
}

#[cfg(test)]
mod tests {
    use crate::{instant_text::instant_text, served_instant::served_instant};
    use pretty_assertions::assert_eq;

    fn read(text: &str) -> Option<String> {
        served_instant(text).map(instant_text)
    }

    #[test]
    fn an_instant_is_read_in_utc() {
        assert_eq!(read("2026-11-12T12:00:00+02:00").as_deref(), Some("2026-11-12T10:00:00Z"));
    }

    #[test]
    fn fractions_are_truncated_to_microseconds_and_a_zero_fraction_is_left_out() {
        assert_eq!(read("2026-11-12T10:00:00.123456789Z").as_deref(), Some("2026-11-12T10:00:00.123456Z"));
        assert_eq!(read("2026-11-12T11:00:00.000Z").as_deref(), Some("2026-11-12T11:00:00Z"));
    }

    #[test]
    fn a_text_without_a_time_or_an_offset_names_no_instant() {
        for text in ["2026-11-12", "2026-11-12T10:00:00", "PT1H", "soon"] {
            assert_eq!(read(text), None, "{text}");
        }
    }
}
