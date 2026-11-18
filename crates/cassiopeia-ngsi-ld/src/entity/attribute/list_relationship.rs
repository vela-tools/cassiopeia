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

/// An NGSI-LD `ListRelationship` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.22).
///
/// Like every reified attribute, a `ListRelationship` may carry nested sub-attributes (ETSI GS CIM
/// 009 v1.9.1 clause 4.5.2.2): a Property or Relationship qualifying the list as a whole. The
/// `attributes` map holds them, flattened into the same JSON object as the list's own members. Its
/// `Eq` derive is dropped because a sub-attribute's value is not `Eq`; `PartialEq` is kept.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, TypedBuilder)]
pub struct NgsiLdListRelationship {
    /// The URNs of the target entities.
    #[serde(rename = "objectList")]
    pub object_list: Vec<Urn>,
    /// The type name of the target entities.
    #[serde(rename = "objectType", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub object_type: Option<NameBuf>,
    /// When it was observed.
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub observed_at: Option<DateTime<Utc>>,
    /// When it ceases to hold (ETSI GS CIM 009 v1.9.1 clause 4.22).
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// The dataset this instance belongs to (ETSI GS CIM 009 v1.9.1 clause 4.5.5.1), set when one
    /// attribute name carries several `ListRelationship` instances (clause 4.5.5), or when the
    /// mapping names one for the single instance.
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub dataset_id: Option<DatasetId>,
    /// Nested sub-attributes qualifying the list (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2).
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    #[builder(default)]
    pub attributes: NestedAttributes,
}

impl NgsiLdListRelationship {
    /// Builds the attribute from its required value, with no qualifiers.
    #[must_use]
    pub fn new(object_list: Vec<Urn>) -> NgsiLdListRelationship {
        NgsiLdListRelationship::builder().object_list(object_list).build()
    }
}

impl SerializeRepr for NgsiLdListRelationship {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_list_relationship(self, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_list_relationship(self, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_list_relationship(self, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::{attribute::list_relationship::NgsiLdListRelationship, dataset_id::DatasetId};
    use pretty_assertions::assert_eq;
    use urn_rs::Urn;

    #[test]
    fn a_new_list_relationship_holds_its_objects() {
        let rel = NgsiLdListRelationship::new(vec!["urn:ngsi-ld:A:1".parse::<Urn>().unwrap()]);
        assert_eq!(rel.object_list.len(), 1);
        assert_eq!(rel.object_type, None);
        assert_eq!(rel.dataset_id, None);
    }

    #[test]
    fn a_dataset_id_is_set_by_direct_field_assignment() {
        let mut rel = NgsiLdListRelationship::new(vec!["urn:ngsi-ld:A:1".parse::<Urn>().unwrap()]);
        rel.dataset_id = Some(DatasetId::new("urn:ngsi-ld:dataset:role:departure").unwrap());

        let value = serde_json::to_value(&rel).unwrap();
        assert_eq!(value["datasetId"], "urn:ngsi-ld:dataset:role:departure");
    }
}
