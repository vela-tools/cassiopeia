use crate::{
    entity::{
        attribute::NestedAttributes,
        representation::{SerializeRepr, concise, normalized, simplified},
    },
    value::json_payload::JsonPayload,
};
use cassiopeia_common::representation::NgsiLdRepresentation;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use urn_rs::Urn;

/// An NGSI-LD `JsonProperty` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.24).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NgsiLdJsonProperty {
    /// The raw JSON object, or array of objects, carried verbatim (clause 4.5.24.2).
    #[serde(rename = "json")]
    pub has_json: JsonPayload,
    /// When it was observed.
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// When it ceases to hold (ETSI GS CIM 009 v1.9.1 clause 4.22).
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
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
    pub fn new(has_json: JsonPayload) -> NgsiLdJsonProperty {
        NgsiLdJsonProperty {
            has_json,
            observed_at: None,
            expires_at: None,
            dataset_id: None,
            attributes: NestedAttributes::default(),
        }
    }
}

impl SerializeRepr for NgsiLdJsonProperty {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_json_property(self, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_json_property(self, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_json_property(self, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::{
            NgsiLdEntity,
            attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper, json_property::NgsiLdJsonProperty},
            name::NameBuf,
            representation::NgsiLdSerializable,
        },
        value::json_payload::JsonPayload,
    };
    use cassiopeia_common::representation::NgsiLdRepresentation;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use urn_rs::Urn;

    fn entity_carrying(payload: serde_json::Value) -> NgsiLdEntity {
        let id: Urn = "urn:ngsi-ld:Sensor:1".parse().expect("valid urn");
        let property = NgsiLdJsonProperty::new(JsonPayload::try_from(payload).expect("an object"));
        NgsiLdEntity::builder(id, NameBuf::new("Sensor").expect("valid name"))
            .attribute(
                NameBuf::new("payload").expect("valid name"),
                NgsiLdAttributeWrapper::single(NgsiLdAttribute::JsonProperty(property)),
            )
            .build()
    }

    #[test]
    fn a_new_json_property_holds_its_payload_and_no_qualifiers() {
        let property = NgsiLdJsonProperty::new(JsonPayload::try_from(json!({"a": 1})).unwrap());

        assert_eq!(property.has_json, JsonPayload::try_from(json!({"a": 1})).unwrap());
        assert_eq!(property.observed_at, None);
        assert!(property.attributes.is_empty());
    }

    #[test]
    fn a_null_nested_in_the_json_member_is_kept_under_json_in_every_representation() {
        let entity = entity_carrying(json!({"maybe": null}));

        let normalized = entity.to_json(NgsiLdRepresentation::Normalized).unwrap();
        assert_eq!(normalized["payload"], json!({"type": "JsonProperty", "json": {"maybe": null}}));

        let concise = entity.to_json(NgsiLdRepresentation::Concise).unwrap();
        assert_eq!(concise["payload"], json!({"json": {"maybe": null}}));

        let simplified = entity.to_json(NgsiLdRepresentation::Simplified).unwrap();
        assert_eq!(simplified["payload"], json!({"json": {"maybe": null}}));
    }
}
