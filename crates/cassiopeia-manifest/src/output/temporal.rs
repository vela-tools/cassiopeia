use getset::Getters;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// The temporal output shape (ETSI GS CIM 009 v1.9.1 clause 5.2.20).
///
/// Absent from the output section means current-state: each id is written once, carrying the latest
/// observation of every attribute with `observedAt` kept as a qualifier. A present block selects a
/// richer temporal shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct ManifestTemporal {
    /// How temporal attributes are represented. `series` folds each id's observations into one
    /// `EntityTemporal` whose temporal attributes are instance arrays.
    #[getset(get = "pub")]
    representation: TemporalRepresentation,
}

/// How temporal attributes are represented on output.
///
/// Serialized in kebab-case, per the crate's convention for internally defined enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display, strum::EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum TemporalRepresentation {
    /// Full instance-array time series per attribute (ETSI GS CIM 009 v1.9.1 clause 5.2.20).
    Series,
}

#[cfg(test)]
mod tests {
    use crate::output::temporal::{ManifestTemporal, TemporalRepresentation};

    #[test]
    fn reads_a_series_representation() {
        let temporal: ManifestTemporal = serde_json::from_str(r#"{"representation": "series"}"#).unwrap();
        assert_eq!(temporal.representation(), &TemporalRepresentation::Series);
    }

    #[test]
    fn a_series_representation_round_trips() {
        let temporal = ManifestTemporal::builder().representation(TemporalRepresentation::Series).build();
        let encoded = serde_json::to_string(&temporal).unwrap();
        assert!(encoded.contains("series"));
        assert_eq!(serde_json::from_str::<ManifestTemporal>(&encoded).unwrap(), temporal);
    }

    #[test]
    fn the_representation_parses_from_its_kebab_case_token() {
        assert_eq!("series".parse::<TemporalRepresentation>().unwrap(), TemporalRepresentation::Series);
        assert_eq!(TemporalRepresentation::Series.to_string(), "series");
    }
}
