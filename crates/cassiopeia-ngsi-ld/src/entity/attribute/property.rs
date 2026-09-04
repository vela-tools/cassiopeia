use crate::{
    entity::{
        attribute::NestedAttributes,
        representation::{SerializeRepr, concise, normalized, simplified},
    },
    value::types::Value,
};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use urn_rs::Urn;

/// An NGSI-LD `Property` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.2).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NgsiLdProperty {
    /// The property's value.
    pub value: Value,
    /// When the value was observed, if temporal.
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// The UN/CEFACT unit code the value is expressed in.
    #[serde(rename = "unitCode", skip_serializing_if = "Option::is_none")]
    pub unit_code: Option<UnitCode>,
    /// The dataset this instance belongs to (NGSI-LD 4.5.5: a URI).
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Urn>,
    /// The broker-assigned instance identifier (NGSI-LD 5.2.5: a URI).
    #[serde(rename = "instanceId", skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<Urn>,
    /// Nested sub-attributes.
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: NestedAttributes,
}

impl NgsiLdProperty {
    pub fn new(value: impl Into<Value>) -> Self {
        Self {
            value: value.into(),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: NestedAttributes::default(),
        }
    }
}

impl SerializeRepr for NgsiLdProperty {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_property(self, skip_null, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_property(self, skip_null, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_property(self, skip_null, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity::attribute::property::NgsiLdProperty, value::types::Value};
    use serde_json::json;

    #[test]
    fn a_new_property_holds_its_value_and_no_qualifiers() {
        let property = NgsiLdProperty::new(Value::from(json!(42)));
        assert!(property.observed_at.is_none());
        assert!(property.unit_code.is_none());
        assert!(property.attributes.is_empty());
    }
}
