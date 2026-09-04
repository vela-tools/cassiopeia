use crate::entity::{
    attribute::NestedAttributes,
    representation::{SerializeRepr, concise, normalized, simplified},
};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize, Serializer};
use urn_rs::Urn;

/// An NGSI-LD `VocabProperty` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.20).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NgsiLdVocabProperty {
    /// The vocabulary IRI this term maps to.
    #[serde(rename = "vocab")]
    pub has_vocab: IriBuf,
    /// When it was observed.
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// The dataset this instance belongs to (a URI).
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Urn>,
    /// Nested sub-attributes.
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: NestedAttributes,
}

impl NgsiLdVocabProperty {
    /// Builds a vocab property mapping to the given vocabulary IRI.
    #[must_use]
    pub fn new(has_vocab: IriBuf) -> NgsiLdVocabProperty {
        NgsiLdVocabProperty {
            has_vocab,
            observed_at: None,
            dataset_id: None,
            attributes: NestedAttributes::default(),
        }
    }
}

impl SerializeRepr for NgsiLdVocabProperty {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_vocab_property(self, skip_null, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_vocab_property(self, skip_null, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_vocab_property(self, skip_null, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::attribute::vocab_property::NgsiLdVocabProperty;
    use iri_rs::IriBuf;

    #[test]
    fn a_new_vocab_property_holds_its_iri() {
        let property = NgsiLdVocabProperty::new(IriBuf::new("urn:ngsi-ld:vocab:x".to_string()).unwrap());
        assert_eq!(property.has_vocab.to_string(), "urn:ngsi-ld:vocab:x");
    }
}
