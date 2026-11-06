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
        "'{rejected}' is not a legal NGSI-LD name: a name starts with a Unicode letter and continues with Unicode letters, numbers or underscores, optionally prefixed by one such segment and a colon (ETSI GS CIM 009 v1.9.1 clause 4.6.2)"
    )]
    InvalidAttributeName {
        /// The value that was rejected.
        rejected: Box<str>,
    },

    /// A value was not a legal NGSI-LD scope.
    #[error(
        "'{rejected}' is not a legal NGSI-LD scope: a scope is one or more '/'-separated levels, each starting with a Unicode letter and continuing with Unicode letters, numbers or underscores (ETSI GS CIM 009 v1.9.1 clause 4.18)"
    )]
    InvalidScope {
        /// The value that was rejected.
        rejected: Box<str>,
    },

    /// The scope of an entity being written was the NGSI-LD Null, which only marks a deleted scope.
    #[error(
        "'urn:ngsi-ld:null' cannot be the scope of an entity Cassiopeia writes: the NGSI-LD Null only marks a deleted scope (ETSI GS CIM 009 v1.9.1 clause 4.18)"
    )]
    NullScope,

    /// A `scope` member was an empty array.
    #[error("a scope array must hold at least one scope: an entity with no scope omits the member (ETSI GS CIM 009 v1.9.1 clause 4.18)")]
    EmptyScopeList,

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
    use pretty_assertions::assert_eq;

    #[test]
    fn an_illegal_name_is_described_by_the_unicode_categories_of_clause_4_6_2() {
        let invalid = NgsiLdError::InvalidAttributeName { rejected: "1bad".into() };

        assert_eq!(
            invalid.to_string(),
            "'1bad' is not a legal NGSI-LD name: a name starts with a Unicode letter and continues with Unicode letters, numbers or underscores, optionally prefixed by one such segment and a colon (ETSI GS CIM 009 v1.9.1 clause 4.6.2)"
        );
    }

    #[test]
    fn an_illegal_scope_is_described_by_its_levels_and_the_unicode_categories_of_clause_4_18() {
        let invalid = NgsiLdError::InvalidScope {
            rejected: "/Novo-mesto".into(),
        };

        assert_eq!(
            invalid.to_string(),
            "'/Novo-mesto' is not a legal NGSI-LD scope: a scope is one or more '/'-separated levels, each starting with a Unicode letter and continuing with Unicode letters, numbers or underscores (ETSI GS CIM 009 v1.9.1 clause 4.18)"
        );
    }

    #[test]
    fn a_null_scope_is_described_as_marking_only_a_deleted_scope_by_clause_4_18() {
        assert_eq!(
            NgsiLdError::NullScope.to_string(),
            "'urn:ngsi-ld:null' cannot be the scope of an entity Cassiopeia writes: the NGSI-LD Null only marks a deleted scope (ETSI GS CIM 009 v1.9.1 clause 4.18)"
        );
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
