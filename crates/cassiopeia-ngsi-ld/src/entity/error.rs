use cassiopeia_geometry::error::GeometryError;
use std::result::Result as StdResult;
use strum::Display;
use thiserror::Error;
use urn_rs::Error as UrnError;

/// A member an NGSI-LD attribute must carry to be well formed.
///
/// The names are spec-facing: ETSI GS CIM 009 v1.9.1 writes them in camelCase, and an error that
/// names one has to name it exactly as the specification does so a reader can find the clause.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
#[strum(serialize_all = "camelCase")]
pub enum MandatoryMember {
    /// A `ListRelationship`'s targets (clause 4.5.22).
    ObjectList,
    /// A `LanguageProperty`'s per-language values (clause 4.5.18).
    LanguageMap,
}

/// Failures raised while constructing, validating, or serializing NGSI-LD entities.
#[derive(Error, Debug)]
pub enum NgsiLdError {
    /// A value was not a valid URN.
    #[error("The value is not a valid URN")]
    InvalidUrn(#[from] UrnError),

    /// A value was not a legal NGSI-LD attribute or type name.
    ///
    /// The rejected text is kept as-is: it is unvalidated input, and giving it the type it failed to
    /// become would be a lie.
    #[error(
        "'{rejected}' is not a legal NGSI-LD name: a name starts with a letter and continues with letters, digits or underscores, optionally prefixed by one such segment and a colon"
    )]
    InvalidAttributeName {
        /// The value that was rejected.
        rejected: Box<str>,
    },

    /// A value was not a legal NGSI-LD scope.
    #[error(
        "'{rejected}' is not a legal NGSI-LD scope: a scope is one or more '/'-separated segments, each starting with a letter (ETSI GS CIM 009 v1.9.1 clause 4.18)"
    )]
    InvalidScope {
        /// The value that was rejected.
        rejected: Box<str>,
    },

    /// A Smart Data Model repository qualifier was malformed.
    #[error(
        "'{rejected}' is not a legal Smart Data Models repository qualifier: a qualifier is one or more '.'-separated alphanumeric segments, each starting with a letter"
    )]
    InvalidDataModelRepository {
        /// The value that was rejected.
        rejected: Box<str>,
    },

    /// An entity or attribute could not be serialized to JSON.
    #[error("The entity or attribute could not be serialized to JSON")]
    SerializationError(#[from] serde_json::Error),

    /// A mandatory member was missing during validation.
    #[error("The mandatory '{member}' member is missing")]
    MissingMandatoryField {
        /// The member the attribute must carry.
        member: MandatoryMember,
    },

    /// A `GeoProperty`'s geometry broke one of RFC 7946 clause 3.1's structural rules.
    #[error("The GeoProperty's geometry is not admissible")]
    InvalidGeometry(#[from] GeometryError),
}

/// Convenience alias for results produced by the NGSI-LD entity layer.
pub type Result<T> = StdResult<T, NgsiLdError>;

#[cfg(test)]
mod tests {
    use crate::entity::error::{MandatoryMember, NgsiLdError};

    #[test]
    fn each_variant_renders_a_message_naming_the_offending_value() {
        let invalid = NgsiLdError::InvalidAttributeName { rejected: "1bad".into() };
        assert!(invalid.to_string().contains("1bad"));
        assert!(invalid.to_string().contains("starts with a letter"));
    }

    #[test]
    fn a_missing_mandatory_member_is_named_as_the_specification_writes_it() {
        let missing = NgsiLdError::MissingMandatoryField {
            member: MandatoryMember::ObjectList,
        };

        assert!(missing.to_string().contains("objectList"));
        assert_eq!(MandatoryMember::LanguageMap.to_string(), "languageMap");
    }

    #[test]
    fn a_serde_json_error_converts_into_a_serialization_error() {
        let json_error = serde_json::from_str::<serde_json::Value>("{ oops").unwrap_err();
        assert!(matches!(NgsiLdError::from(json_error), NgsiLdError::SerializationError(_)));
    }
}
