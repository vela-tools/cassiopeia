use std::borrow::Cow;

/// How the values of a time coordinate relate to the unit the CF decoder is handed.
///
/// The decoder spells only `second`, `minute`, `hour`, `day` and `month` and their common
/// abbreviations, while CF 1.11 §4.4 accepts every UDUNITS time unit and names `sec` itself. The other
/// spellings are rewritten to a unit the decoder knows, scaling the values where the unit differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnitScale {
    /// The values count the decoder's own unit.
    Native,
    /// The values count weeks, decoded as days.
    Weeks,
    /// The values count milliseconds, decoded as seconds.
    Milliseconds,
    /// The values count microseconds, decoded as seconds.
    Microseconds,
}

impl TimeUnitScale {
    /// Rewrites CF time units `<unit> since <epoch>` whose unit the decoder does not spell into units it
    /// does, returning the units to decode with and how values relate to them. Units the decoder reads
    /// already, and units it rejects anyway, are returned unchanged. Folding ASCII case changes no
    /// byte, so the split found in the folded units falls on the same boundary of the original.
    #[must_use]
    pub fn rewrite(units: &str) -> (Cow<'_, str>, TimeUnitScale) {
        let Some(since) = units.to_ascii_lowercase().find(" since ") else {
            return (Cow::Borrowed(units), TimeUnitScale::Native);
        };
        let (unit, epoch) = units.split_at(since);
        let (decoder_unit, scale) = match unit.trim().to_ascii_lowercase().as_str() {
            "sec" | "secs" => ("seconds", TimeUnitScale::Native),
            "mins" => ("minutes", TimeUnitScale::Native),
            "hrs" => ("hours", TimeUnitScale::Native),
            "week" | "weeks" => ("days", TimeUnitScale::Weeks),
            "millisecond" | "milliseconds" | "msec" | "msecs" | "ms" => ("seconds", TimeUnitScale::Milliseconds),
            "microsecond" | "microseconds" | "usec" | "usecs" | "us" => ("seconds", TimeUnitScale::Microseconds),
            _decoder_spelling => return (Cow::Borrowed(units), TimeUnitScale::Native),
        };
        (Cow::Owned(format!("{decoder_unit}{epoch}")), scale)
    }

    /// The value counted in the decoder's unit.
    #[must_use]
    pub fn to_decoder_unit(self, value: f64) -> f64 {
        match self {
            TimeUnitScale::Native => value,
            TimeUnitScale::Weeks => value * 7.0,
            TimeUnitScale::Milliseconds => value / 1e3,
            TimeUnitScale::Microseconds => value / 1e6,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::time_unit::TimeUnitScale;
    use pretty_assertions::assert_eq;

    fn rewritten(units: &str) -> (String, TimeUnitScale) {
        let (units, scale) = TimeUnitScale::rewrite(units);
        (units.into_owned(), scale)
    }

    #[test]
    fn the_cf_sec_abbreviation_becomes_seconds() {
        assert_eq!(
            rewritten("sec since 1970-01-01"),
            ("seconds since 1970-01-01".to_string(), TimeUnitScale::Native)
        );
    }

    #[test]
    fn spellings_the_decoder_reads_are_left_alone() {
        assert_eq!(
            rewritten("Hour since 2026-11-03T00:00:00Z"),
            ("Hour since 2026-11-03T00:00:00Z".to_string(), TimeUnitScale::Native)
        );
        assert_eq!(rewritten("K"), ("K".to_string(), TimeUnitScale::Native));
    }

    #[test]
    fn weeks_become_days_scaled_by_seven() {
        let (units, scale) = rewritten("weeks since 2026-01-01");
        assert_eq!(units, "days since 2026-01-01");
        assert_eq!(scale.to_decoder_unit(2.0).to_bits(), 14.0_f64.to_bits());
    }

    #[test]
    fn milliseconds_and_microseconds_become_seconds() {
        let (units, milliseconds) = rewritten("milliseconds since 1970-01-01T00:00:00Z");
        assert_eq!(units, "seconds since 1970-01-01T00:00:00Z");
        assert_eq!(milliseconds.to_decoder_unit(1_500.0).to_bits(), 1.5_f64.to_bits());
        let (units, microseconds) = rewritten("us since 2000-01-01");
        assert_eq!(units, "seconds since 2000-01-01");
        assert_eq!(microseconds.to_decoder_unit(-250_000.0).to_bits(), (-0.25_f64).to_bits());
    }
}
