use crate::entity::{
    attribute::NestedAttributes,
    dataset_id::DatasetId,
    name::NameBuf,
    representation::{SerializeRepr, concise, normalized, simplified},
};
use cassiopeia_common::representation::NgsiLdRepresentation;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use typed_builder::TypedBuilder;
use urn_rs::Urn;

/// An NGSI-LD `Relationship` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.3).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, TypedBuilder)]
pub struct NgsiLdRelationship {
    /// The URN of the entity this relationship points at.
    pub object: Urn,
    /// The type name of the target entity.
    #[serde(rename = "objectType", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub object_type: Option<NameBuf>,
    /// When the relationship was observed.
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub observed_at: Option<DateTime<Utc>>,
    /// When the relationship ceases to hold (ETSI GS CIM 009 v1.9.1 clause 4.22).
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// The dataset this instance belongs to (ETSI GS CIM 009 v1.9.1 clause 4.5.5.1).
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub dataset_id: Option<DatasetId>,
    /// The broker-assigned instance identifier (a URI).
    #[serde(rename = "instanceId", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub instance_id: Option<Urn>,
    /// Nested sub-attributes.
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    #[builder(default)]
    pub attributes: NestedAttributes,
}

impl NgsiLdRelationship {
    /// Builds a relationship pointing at `object` with no qualifiers.
    #[must_use]
    pub fn new(object: Urn) -> NgsiLdRelationship {
        NgsiLdRelationship::builder().object(object).build()
    }
}

impl SerializeRepr for NgsiLdRelationship {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_relationship(self, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_relationship(self, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_relationship(self, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::attribute::relationship::NgsiLdRelationship;
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    #[test]
    fn a_new_relationship_carries_only_its_object() {
        let rel = NgsiLdRelationship::new("urn:ngsi-ld:Building:1".parse::<Urn>().unwrap());
        assert_eq!(rel.object_type, None);
        assert!(rel.attributes.is_empty());
    }
}
