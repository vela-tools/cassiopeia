use crate::entity::{
    attribute::NestedAttributes,
    name::NameBuf,
    representation::{SerializeRepr, concise, normalized, simplified},
};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use urn_rs::Urn;

/// An NGSI-LD `Relationship` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.3).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NgsiLdRelationship {
    /// The URN of the entity this relationship points at.
    pub object: Urn,
    /// The type name of the target entity.
    #[serde(rename = "objectType", skip_serializing_if = "Option::is_none")]
    pub object_type: Option<NameBuf>,
    /// When the relationship was observed.
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// The dataset this instance belongs to (a URI).
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Urn>,
    /// The broker-assigned instance identifier (a URI).
    #[serde(rename = "instanceId", skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<Urn>,
    /// Nested sub-attributes.
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: NestedAttributes,
}

impl NgsiLdRelationship {
    /// Builds a relationship pointing at `object` with no qualifiers.
    #[must_use]
    pub fn new(object: Urn) -> Self {
        Self {
            object,
            object_type: None,
            observed_at: None,
            dataset_id: None,
            instance_id: None,
            attributes: NestedAttributes::default(),
        }
    }
}

impl SerializeRepr for NgsiLdRelationship {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_relationship(self, skip_null, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_relationship(self, skip_null, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_relationship(self, skip_null, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::attribute::relationship::NgsiLdRelationship;
    use urn_rs::Urn;

    #[test]
    fn a_new_relationship_carries_only_its_object() {
        let rel = NgsiLdRelationship::new("urn:ngsi-ld:Building:1".parse::<Urn>().unwrap());
        assert!(rel.object_type.is_none());
        assert!(rel.attributes.is_empty());
    }
}
