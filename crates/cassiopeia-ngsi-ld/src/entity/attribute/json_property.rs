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

/// An NGSI-LD `JsonProperty` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.24).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NgsiLdJsonProperty {
    /// The raw JSON value carried verbatim.
    #[serde(rename = "json")]
    pub has_json: Value,
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

impl NgsiLdJsonProperty {
    /// Builds the attribute from its required value, with no qualifiers.
    #[must_use]
    pub fn new(has_json: impl Into<Value>) -> Self {
        Self {
            has_json: has_json.into(),
            observed_at: None,
            dataset_id: None,
            attributes: NestedAttributes::default(),
        }
    }
}

impl SerializeRepr for NgsiLdJsonProperty {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_json_property(self, skip_null, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_json_property(self, skip_null, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_json_property(self, skip_null, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity::attribute::json_property::NgsiLdJsonProperty, value::types::Value};
    use serde_json::json;

    #[test]
    fn a_new_json_property_holds_its_value() {
        let property = NgsiLdJsonProperty::new(Value::from(json!({"a": 1})));
        assert!(!property.has_json.is_null());
    }
}
