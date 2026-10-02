use cassiopeia_ngsi_ld::entity::error::NgsiLdError;
use std::num::ParseIntError;
use thiserror::Error;

/// Why the text form of a [`RelationshipKey`](crate::relationship_key::RelationshipKey) could not be
/// read back.
#[derive(Debug, Error)]
pub enum RelationshipKeyError {
    /// An attribute-name segment of the key is not a legal NGSI-LD name (ETSI GS CIM 009 v1.9.1
    /// clause 4.6.2).
    #[error("A relationship key names an attribute that is not a legal NGSI-LD name")]
    Name(#[from] NgsiLdError),

    /// The instance part of an instance key is not a declaration index.
    #[error("The relationship key instance '{rejected}' is not an instance index")]
    InstanceIndex {
        /// The rejected instance text, kept as-is because it is unvalidated input.
        rejected: Box<str>,
        /// Why it is not an index.
        #[source]
        source: ParseIntError,
    },
}

#[cfg(test)]
mod tests {
    use crate::relationship_key_error::RelationshipKeyError;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;

    #[test]
    fn an_illegal_name_converts_into_the_name_variant() {
        let error: RelationshipKeyError = NameBuf::new("9bad").expect_err("must reject").into();

        assert!(matches!(error, RelationshipKeyError::Name(_)));
    }
}
