use crate::entity::{
    attribute::NestedAttributes,
    dataset_id::DatasetId,
    representation::{SerializeRepr, concise, normalized, simplified},
    value_type::ValueType,
};
use cassiopeia_common::representation::NgsiLdRepresentation;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize, Serializer};
use typed_builder::TypedBuilder;

/// An NGSI-LD `VocabProperty` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.20).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, TypedBuilder)]
pub struct NgsiLdVocabProperty {
    /// The vocabulary IRI this term maps to.
    #[serde(rename = "vocab")]
    pub has_vocab: IriBuf,
    /// The datatype the mapping declared for the value (ETSI GS CIM 009 v1.9.1 table 5.2.35-1).
    #[serde(rename = "valueType", default, skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub value_type: Option<ValueType>,
    /// When it was observed.
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub observed_at: Option<DateTime<Utc>>,
    /// When it ceases to hold (ETSI GS CIM 009 v1.9.1 clause 4.22).
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// The dataset this instance belongs to (ETSI GS CIM 009 v1.9.1 clause 4.5.5.1).
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub dataset_id: Option<DatasetId>,
    /// Nested sub-attributes.
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    #[builder(default)]
    pub attributes: NestedAttributes,
}

impl NgsiLdVocabProperty {
    /// Builds a vocab property mapping to the given vocabulary IRI.
    #[must_use]
    pub fn new(has_vocab: IriBuf) -> NgsiLdVocabProperty {
        NgsiLdVocabProperty::builder().has_vocab(has_vocab).build()
    }
}

impl SerializeRepr for NgsiLdVocabProperty {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_vocab_property(self, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_vocab_property(self, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_vocab_property(self, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::attribute::vocab_property::NgsiLdVocabProperty;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_new_vocab_property_holds_its_iri() {
        let property = NgsiLdVocabProperty::new(IriBuf::new("urn:ngsi-ld:vocab:x".to_string()).unwrap());
        assert_eq!(property.has_vocab.to_string(), "urn:ngsi-ld:vocab:x");
    }
}
