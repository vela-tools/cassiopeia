use crate::{
    entity::{
        attribute::NestedAttributes,
        representation::{SerializeRepr, concise, normalized, simplified},
    },
    value::types::Value,
};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use urn_rs::Urn;

/// An NGSI-LD `ListProperty` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.21).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NgsiLdListProperty {
    /// The ordered list of values.
    #[serde(rename = "valueList")]
    pub has_value_list: Vec<Value>,
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

impl NgsiLdListProperty {
    /// Builds the attribute from its required value, with no qualifiers.
    #[must_use]
    pub fn new(has_value_list: Vec<Value>) -> Self {
        Self {
            has_value_list,
            observed_at: None,
            dataset_id: None,
            attributes: NestedAttributes::default(),
        }
    }
}

impl SerializeRepr for NgsiLdListProperty {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_list_property(self, skip_null, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_list_property(self, skip_null, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_list_property(self, skip_null, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity::attribute::list_property::NgsiLdListProperty, value::types::Value};
    use serde_json::json;

    #[test]
    fn a_new_list_property_holds_its_values() {
        let property = NgsiLdListProperty::new(vec![Value::from(json!(1)), Value::from(json!(2))]);
        assert_eq!(property.has_value_list.len(), 2);
    }
}
