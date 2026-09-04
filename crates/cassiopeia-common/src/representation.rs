use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// The NGSI-LD representation an entity is serialized in.
///
/// See ETSI GS CIM 009 v1.9.1, clause 4.5, for the shape each representation produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum NgsiLdRepresentation {
    /// Fully expanded representation carrying every attribute's type and metadata.
    ///
    /// This is the domain `Default`: Cassiopeia writes Normalized unless a run overrides it. The
    /// validation stage does not read this default: it defaults to Simplified, resolved in the
    /// pipeline's `from_manifest` from the manifest's `output.validation` (or a `--validation-*`
    /// flag), so changing this value never affects what representation entities are validated in.
    #[default]
    Normalized,
    /// Compact representation omitting redundant type metadata.
    Concise,
    /// Flat key-value representation without metadata.
    Simplified,
}

#[cfg(test)]
mod tests {
    use crate::representation::NgsiLdRepresentation;

    #[test]
    fn every_representation_round_trips_through_its_lowercase_token() {
        for (representation, token) in [
            (NgsiLdRepresentation::Normalized, r#""normalized""#),
            (NgsiLdRepresentation::Concise, r#""concise""#),
            (NgsiLdRepresentation::Simplified, r#""simplified""#),
        ] {
            assert_eq!(serde_json::to_string(&representation).unwrap(), token);
            assert_eq!(serde_json::from_str::<NgsiLdRepresentation>(token).unwrap(), representation);
        }
    }

    #[test]
    fn the_default_representation_is_normalized() {
        assert_eq!(NgsiLdRepresentation::default(), NgsiLdRepresentation::Normalized);
    }
}
