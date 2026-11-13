use getset::Getters;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// The temporal output shape (ETSI GS CIM 009 v1.9.1 clause 5.2.20).
///
/// Absent from the output section means current-state: each id is written once, holding per mapping
/// the record with the latest `observedAt`, kept as a qualifier, unless a Context Broker destination's
/// `temporal` operation sends each observation on its own. A present block selects a richer temporal
/// shape. A key the block does not define is refused, so a misspelt one is an error
/// rather than silently ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters, TypedBuilder)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    use pretty_assertions::assert_eq;

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
    fn a_temporal_block_with_a_misspelt_key_is_rejected_as_an_unknown_field() {
        let error = serde_json::from_str::<ManifestTemporal>(r#"{"representation": "series", "representaton": "series"}"#).unwrap_err();

        assert!(error.to_string().contains("unknown field `representaton`"), "{error}");
    }

    #[test]
    fn the_representation_parses_from_its_kebab_case_token() {
        assert_eq!("series".parse::<TemporalRepresentation>().unwrap(), TemporalRepresentation::Series);
        assert_eq!(TemporalRepresentation::Series.to_string(), "series");
    }
}
