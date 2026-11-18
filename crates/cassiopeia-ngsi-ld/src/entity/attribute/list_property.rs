use crate::{
    entity::{
        attribute::NestedAttributes,
        dataset_id::DatasetId,
        representation::{SerializeRepr, concise, normalized, simplified},
        value_type::ValueType,
    },
    value::types::Value,
};
use cassiopeia_common::representation::NgsiLdRepresentation;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use typed_builder::TypedBuilder;

/// An NGSI-LD `ListProperty` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.21).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, TypedBuilder)]
pub struct NgsiLdListProperty {
    /// The ordered list of values.
    #[serde(rename = "valueList")]
    pub has_value_list: Vec<Value>,
    /// The datatype the mapping declared for the value (ETSI GS CIM 009 v1.9.1 table 5.2.36-1).
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

impl NgsiLdListProperty {
    /// Builds the attribute from its required value, with no qualifiers.
    #[must_use]
    pub fn new(has_value_list: Vec<Value>) -> NgsiLdListProperty {
        NgsiLdListProperty::builder().has_value_list(has_value_list).build()
    }
}

impl SerializeRepr for NgsiLdListProperty {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_list_property(self, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_list_property(self, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_list_property(self, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity::attribute::list_property::NgsiLdListProperty, value::types::Value};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn a_new_list_property_holds_its_values() {
        let property = NgsiLdListProperty::new(vec![Value::try_from(json!(1)).unwrap(), Value::try_from(json!(2)).unwrap()]);
        assert_eq!(property.has_value_list.len(), 2);
    }
}
