use chrono::{DateTime, SecondsFormat, Utc};

/// `instant` written in RFC 3339, in UTC with a `Z`, with a fraction only when it has one, in groups
/// of three digits.
///
/// A reading's time and the bounds a range read requests are both written this way, so the instants
/// a snapshot, a record and a request hold look alike.
#[must_use]
pub fn instant_text(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

#[cfg(test)]
mod tests {
    use crate::instant_text::instant_text;
    use chrono::DateTime;
    use pretty_assertions::assert_eq;

    fn written(text: &str) -> String {
        instant_text(DateTime::parse_from_rfc3339(text).unwrap().to_utc())
    }

    #[test]
    fn a_whole_second_is_written_without_a_fraction() {
        assert_eq!(written("2026-11-12T10:00:00.000Z"), "2026-11-12T10:00:00Z");
    }

    #[test]
    fn a_fraction_is_written_in_groups_of_three_digits() {
        assert_eq!(written("2026-11-11T10:00:00.25Z"), "2026-11-11T10:00:00.250Z");
        assert_eq!(written("2026-11-11T10:00:00.123456Z"), "2026-11-11T10:00:00.123456Z");
    }

    #[test]
    fn an_offset_is_written_as_utc() {
        assert_eq!(written("2026-11-12T12:00:00+02:00"), "2026-11-12T10:00:00Z");
    }
}
