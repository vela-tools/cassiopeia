use serde::{Deserialize, Serialize};
use strum::Display;

/// The mapping document format version, declared by every mapping's `version` field.
///
/// Only the current version is accepted. A document declaring an unsupported version fails to
/// deserialize rather than being read on a best-effort basis, so a mapping can never be silently
/// interpreted under semantics it did not declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Version {
    /// Version 4: entity identity is declared as `entityName` plus an optional `scope`.
    V4,
}

#[cfg(test)]
mod tests {
    use crate::version::Version;

    #[test]
    fn the_current_version_deserializes_from_its_lowercase_token() {
        assert_eq!(serde_json::from_str::<Version>(r#""v4""#).unwrap(), Version::V4);
    }

    #[test]
    fn the_current_version_serializes_to_its_lowercase_token() {
        assert_eq!(serde_json::to_string(&Version::V4).unwrap(), r#""v4""#);
    }

    #[test]
    fn displays_as_its_wire_token() {
        assert_eq!(Version::V4.to_string(), "v4");
    }

    #[test]
    fn a_superseded_version_is_rejected() {
        assert!(serde_json::from_str::<Version>(r#""v3""#).is_err());
    }
}
