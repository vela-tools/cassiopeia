/// The time-zone designator UDUNITS allows after the epoch of time units such as
/// `hours since 2026-01-01 00:00:00 UTC` or `seconds since 1970-01-01T00:00:00Z`.
///
/// The CF time decoder accepts no designator at all, so a UTC one is removed before decoding and any
/// other offset is refused rather than silently read as UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochZone<'u> {
    /// The epoch names no zone, or one that is UTC; the payload is the units without the designator.
    Utc(&'u str),
    /// The epoch names a non-zero UTC offset, which the decoder cannot apply.
    Offset,
}

impl<'u> EpochZone<'u> {
    /// Splits the zone designator off the epoch of ASCII time `units`.
    ///
    /// The designator is a trailing `Z`, a separate `UTC`, `GMT` or `Z` token, or a `+hh`, `+hhmm` or
    /// `+hh:mm` offset (either sign) attached to the time or separated from it by a space. A separate
    /// offset may also be unsigned, as in the `seconds since 1970-01-01 00:00:00 0:00` the NCEI netCDF
    /// templates prescribe, once a time of day precedes it. Units without ` since ` are returned
    /// unchanged.
    #[must_use]
    pub fn split(units: &'u str) -> EpochZone<'u> {
        let Some(since) = units.to_ascii_lowercase().find(" since ") else {
            return EpochZone::Utc(units);
        };
        let epoch_start = since + " since ".len();
        let trimmed = units.trim_end();
        let Some(zone_start) = trailing_zone(trimmed.get(epoch_start..).unwrap_or_default()).map(|offset| epoch_start + offset) else {
            return EpochZone::Utc(trimmed);
        };
        let zone = trimmed.get(zone_start..).unwrap_or_default();
        let units = trimmed.get(..zone_start).unwrap_or_default().trim_end();
        if is_utc(zone) { EpochZone::Utc(units) } else { EpochZone::Offset }
    }
}

/// Where a trailing zone designator starts in `epoch`, if it ends with one.
fn trailing_zone(epoch: &str) -> Option<usize> {
    // An unsigned token is an offset only after a time of day: in `2026-01-01 06:00` it is the time.
    if let Some((rest, last)) = epoch.rsplit_once(char::is_whitespace)
        && (is_named_zone(last) || is_offset(last) || (rest.contains(':') && is_offset_amount(last)))
    {
        return Some(epoch.len() - last.len());
    }
    if epoch.ends_with(['Z', 'z']) {
        return Some(epoch.len() - 1);
    }
    // An offset attached to the time follows its last digit; the date's own hyphens come before the
    // time, which starts after the `T` or the space separating it from the date.
    let time_start = epoch.find(['T', ' '])? + 1;
    let sign = time_start + epoch.get(time_start..)?.rfind(['+', '-'])?;
    is_offset(epoch.get(sign..)?).then_some(sign)
}

/// Whether `token` names UTC by name.
fn is_named_zone(token: &str) -> bool {
    ["UTC", "GMT", "Z"].iter().any(|zone| zone.eq_ignore_ascii_case(token))
}

/// Whether `token` is a signed `hh`, `hhmm` or `hh:mm` offset.
fn is_offset(token: &str) -> bool {
    token.strip_prefix(['+', '-']).is_some_and(is_offset_amount)
}

/// Whether `amount` is an unsigned `hh`, `hhmm` or `hh:mm` offset.
fn is_offset_amount(amount: &str) -> bool {
    let (hours, minutes) = amount.split_once(':').unwrap_or_else(|| amount.split_at(amount.len().min(2)));
    let all_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    (1..=2).contains(&hours.len()) && all_digits(hours) && (minutes.is_empty() || minutes.len() == 2) && all_digits(minutes)
}

/// Whether a zone designator names UTC: by name, or as an offset of zero, signed or not.
fn is_utc(zone: &str) -> bool {
    is_named_zone(zone) || zone.trim_start_matches(['+', '-']).bytes().all(|byte| byte == b'0' || byte == b':')
}

#[cfg(test)]
mod tests {
    use crate::netcdf::epoch_zone::EpochZone;
    use pretty_assertions::assert_eq;

    #[test]
    fn units_without_a_zone_are_returned_unchanged() {
        assert_eq!(EpochZone::split("days since 2026-01-01"), EpochZone::Utc("days since 2026-01-01"));
        assert_eq!(
            EpochZone::split("hours since 2026-01-01 00:00:00"),
            EpochZone::Utc("hours since 2026-01-01 00:00:00")
        );
        assert_eq!(EpochZone::split("degrees_north"), EpochZone::Utc("degrees_north"));
    }

    #[test]
    fn a_trailing_z_is_removed() {
        assert_eq!(
            EpochZone::split("seconds since 1970-01-01T00:00:00Z"),
            EpochZone::Utc("seconds since 1970-01-01T00:00:00")
        );
    }

    #[test]
    fn a_separate_utc_token_is_removed() {
        assert_eq!(
            EpochZone::split("hours since 2026-01-01 00:00:00 UTC"),
            EpochZone::Utc("hours since 2026-01-01 00:00:00")
        );
        assert_eq!(EpochZone::split("days since 2026-01-01 utc"), EpochZone::Utc("days since 2026-01-01"));
    }

    #[test]
    fn a_zero_offset_is_removed_in_every_spelling() {
        for units in [
            "hours since 2026-01-01T00:00:00+00:00",
            "hours since 2026-01-01T00:00:00+0000",
            "hours since 2026-01-01T00:00:00 -00:00",
            "hours since 2026-01-01T00:00:00+00",
        ] {
            assert_eq!(EpochZone::split(units), EpochZone::Utc("hours since 2026-01-01T00:00:00"), "{units}");
        }
    }

    #[test]
    fn an_unsigned_zero_offset_after_a_time_of_day_is_removed() {
        assert_eq!(
            EpochZone::split("seconds since 1970-01-01 00:00:00 0:00"),
            EpochZone::Utc("seconds since 1970-01-01 00:00:00")
        );
    }

    #[test]
    fn an_unsigned_clock_after_a_bare_date_is_the_time_of_day() {
        assert_eq!(EpochZone::split("hours since 2026-01-01 06:00"), EpochZone::Utc("hours since 2026-01-01 06:00"));
    }

    #[test]
    fn a_non_zero_offset_is_refused() {
        assert_eq!(EpochZone::split("hours since 1990-01-01 00:00:00 6:00"), EpochZone::Offset);
        assert_eq!(EpochZone::split("hours since 2026-01-01T00:00:00+02:00"), EpochZone::Offset);
        assert_eq!(EpochZone::split("hours since 1990-1-1 0:0:0 -6:00"), EpochZone::Offset);
        assert_eq!(EpochZone::split("hours since 2026-01-01T00:00:00-0500"), EpochZone::Offset);
    }

    #[test]
    fn the_hyphens_of_a_date_without_a_time_are_not_an_offset() {
        assert_eq!(EpochZone::split("days since 2026-01-15"), EpochZone::Utc("days since 2026-01-15"));
    }
}
