/// A time coordinate value decoded into the RFC 3339 timestamp a record carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedTime {
    /// A date the proleptic Gregorian calendar has, so the timestamp names the instant RFC 3339 reads
    /// it as.
    Gregorian(String),
    /// A model calendar's date the Gregorian calendar does not have, such as `360_day` February 30,
    /// written in the same form though it names no instant and no RFC 3339 reader accepts it.
    Nonexistent(String),
}

impl DecodedTime {
    /// The timestamp of a date the Gregorian calendar does not have, or `None` for a date it has.
    #[must_use]
    pub const fn nonexistent(&self) -> Option<&str> {
        match self {
            DecodedTime::Gregorian(_) => None,
            DecodedTime::Nonexistent(timestamp) => Some(timestamp.as_str()),
        }
    }

    /// The timestamp, whether or not the Gregorian calendar has its date: a record keeps either.
    #[must_use]
    pub fn into_timestamp(self) -> String {
        match self {
            DecodedTime::Gregorian(timestamp) | DecodedTime::Nonexistent(timestamp) => timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::decoded_time::DecodedTime;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_a_date_the_gregorian_calendar_lacks_is_nonexistent() {
        assert_eq!(DecodedTime::Gregorian("2026-02-28T00:00:00+00:00".to_owned()).nonexistent(), None);
        assert_eq!(
            DecodedTime::Nonexistent("2026-02-30T00:00:00+00:00".to_owned()).nonexistent(),
            Some("2026-02-30T00:00:00+00:00")
        );
    }

    #[test]
    fn either_kind_of_date_gives_up_its_timestamp() {
        assert_eq!(
            DecodedTime::Gregorian("2026-02-28T00:00:00+00:00".to_owned()).into_timestamp(),
            "2026-02-28T00:00:00+00:00"
        );
        assert_eq!(
            DecodedTime::Nonexistent("2026-02-30T00:00:00+00:00".to_owned()).into_timestamp(),
            "2026-02-30T00:00:00+00:00"
        );
    }
}
