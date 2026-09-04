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

/// An NGSI-LD `ListRelationship` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.22).
///
/// Like every reified attribute, a `ListRelationship` may carry nested sub-attributes (ETSI GS CIM
/// 009 v1.9.1 clause 4.5.2.2): a Property or Relationship qualifying the list as a whole. The
/// `attributes` map holds them, flattened into the same JSON object as the list's own members. Its
/// `Eq` derive is dropped because a sub-attribute's value is not `Eq`; `PartialEq` is kept.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NgsiLdListRelationship {
    /// The URNs of the target entities.
    #[serde(rename = "objectList")]
    pub object_list: Vec<Urn>,
    /// The type name of the target entities.
    #[serde(rename = "objectType", skip_serializing_if = "Option::is_none")]
    pub object_type: Option<NameBuf>,
    /// When it was observed.
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// The dataset this instance belongs to (a URI), set when one attribute name carries several
    /// `ListRelationship` instances (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Urn>,
    /// Nested sub-attributes qualifying the list (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2).
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: NestedAttributes,
}

impl NgsiLdListRelationship {
    /// Builds the attribute from its required value, with no qualifiers.
    #[must_use]
    pub fn new(object_list: Vec<Urn>) -> Self {
        Self {
            object_list,
            object_type: None,
            observed_at: None,
            dataset_id: None,
            attributes: NestedAttributes::default(),
        }
    }
}

impl SerializeRepr for NgsiLdListRelationship {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_list_relationship(self, skip_null, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_list_relationship(self, skip_null, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_list_relationship(self, skip_null, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::attribute::list_relationship::NgsiLdListRelationship;
    use urn_rs::Urn;

    #[test]
    fn a_new_list_relationship_holds_its_objects() {
        let rel = NgsiLdListRelationship::new(vec!["urn:ngsi-ld:A:1".parse::<Urn>().unwrap()]);
        assert_eq!(rel.object_list.len(), 1);
        assert!(rel.object_type.is_none());
        assert!(rel.dataset_id.is_none());
    }

    #[test]
    fn a_dataset_id_is_set_by_direct_field_assignment() {
        let mut rel = NgsiLdListRelationship::new(vec!["urn:ngsi-ld:A:1".parse::<Urn>().unwrap()]);
        rel.dataset_id = Some("urn:ngsi-ld:dataset:role:departure".parse::<Urn>().unwrap());

        let value = serde_json::to_value(&rel).unwrap();
        assert_eq!(value["datasetId"], "urn:ngsi-ld:dataset:role:departure");
    }
}
