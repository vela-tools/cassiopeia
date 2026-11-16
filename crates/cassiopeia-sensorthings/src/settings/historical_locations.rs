use serde::{Deserialize, Serialize};

/// Whether a range or incremental read requests the `HistoricalLocations` of every Thing it lists: the
/// `historicalLocations` member of a `sensorthings` block.
///
/// The member is a JSON boolean on the wire, `true` for [`HistoricalLocations::Requested`] and
/// `false` for [`HistoricalLocations::Omitted`]. A Thing's `HistoricalLocations` give the times of its
/// current and previous Locations (OGC 18-088 section 8.2.3), the track of a Thing that moves.
/// Reading them takes requests of their own for every Thing, so a read asks for them only when told
/// to. A latest read reads none: its record's `location` already is the Thing's last known place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(from = "bool", into = "bool")]
pub enum HistoricalLocations {
    /// The `HistoricalLocations` of every Thing the read lists are requested
    /// (`historicalLocations: true`).
    Requested,
    /// No `HistoricalLocation` is requested (`historicalLocations: false`, or no member).
    #[default]
    Omitted,
}

// Written by hand: no derive maps a `bool` onto the variants of a two-variant enum.
impl From<bool> for HistoricalLocations {
    fn from(requested: bool) -> HistoricalLocations {
        if requested {
            HistoricalLocations::Requested
        } else {
            HistoricalLocations::Omitted
        }
    }
}

// Written by hand: no derive maps the variants of a two-variant enum onto a `bool`.
impl From<HistoricalLocations> for bool {
    fn from(historical_locations: HistoricalLocations) -> bool {
        match historical_locations {
            HistoricalLocations::Requested => true,
            HistoricalLocations::Omitted => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::historical_locations::HistoricalLocations;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn a_json_boolean_reads_as_requested_or_omitted() {
        assert_eq!(
            serde_json::from_value::<HistoricalLocations>(json!(true)).unwrap(),
            HistoricalLocations::Requested
        );
        assert_eq!(
            serde_json::from_value::<HistoricalLocations>(json!(false)).unwrap(),
            HistoricalLocations::Omitted
        );
    }

    #[test]
    fn the_setting_is_written_back_as_a_json_boolean() {
        assert_eq!(serde_json::to_value(HistoricalLocations::Requested).unwrap(), json!(true));
        assert_eq!(serde_json::to_value(HistoricalLocations::Omitted).unwrap(), json!(false));
    }

    #[test]
    fn anything_but_a_boolean_is_refused() {
        for value in [json!("true"), json!(1), json!(null)] {
            assert!(serde_json::from_value::<HistoricalLocations>(value.clone()).is_err(), "{value}");
        }
    }

    #[test]
    fn historical_locations_are_omitted_by_default() {
        assert_eq!(HistoricalLocations::default(), HistoricalLocations::Omitted);
    }
}
