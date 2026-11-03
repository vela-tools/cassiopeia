use crate::entity::{
    attribute::{
        geo_property::NgsiLdGeoProperty,
        json_property::NgsiLdJsonProperty,
        language_property::NgsiLdLanguageProperty,
        list_property::NgsiLdListProperty,
        list_relationship::NgsiLdListRelationship,
        property::NgsiLdProperty,
        relationship::NgsiLdRelationship,
        vocab_property::NgsiLdVocabProperty,
    },
    name::NameBuf,
    representation::{ReprAdapter, SerializeRepr},
};
use cassiopeia_common::representation::NgsiLdRepresentation;
use chrono::{DateTime, Utc};
use foldhash::fast::RandomState;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeSeq};
use strum::EnumDiscriminants;

pub mod geo_property;
pub mod json_property;
pub mod language_property;
pub mod list_property;
pub mod list_relationship;
pub mod property;
pub mod relationship;
pub mod vocab_property;

/// An attribute occurrence: a single instance, or several instances that differ by `datasetId`.
///
/// The single instance is boxed to keep the two variants' sizes close (the attribute structs are
/// much larger than a `Vec` handle).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum NgsiLdAttributeWrapper {
    /// A single attribute instance.
    Single(Box<NgsiLdAttribute>),
    /// Several instances of the same attribute, distinguished by `datasetId`.
    Multi(Vec<NgsiLdAttribute>),
}

impl NgsiLdAttributeWrapper {
    /// Wraps a single attribute instance.
    #[must_use]
    pub fn single(attribute: NgsiLdAttribute) -> NgsiLdAttributeWrapper {
        NgsiLdAttributeWrapper::Single(Box::new(attribute))
    }
}

impl SerializeRepr for NgsiLdAttributeWrapper {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation) -> Result<S::Ok, S::Error> {
        match self {
            NgsiLdAttributeWrapper::Single(attr) => attr.serialize_repr(serializer, representation),
            NgsiLdAttributeWrapper::Multi(attrs) => {
                let mut seq = serializer.serialize_seq(None)?;
                for attr in attrs {
                    seq.serialize_element(&ReprAdapter::new(attr, representation))?;
                }
                seq.end()
            }
        }
    }
}

/// The set of attribute types NGSI-LD defines (ETSI GS CIM 009 v1.9.1, clause 4.5).
///
/// `NgsiLdAttributeKind` is derived from `NgsiLdAttribute` rather than declared separately so
/// the tag used to select an attribute type can never drift from the attributes that exist.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, EnumDiscriminants)]
#[strum_discriminants(name(NgsiLdAttributeKind), derive(Deserialize, Serialize, Hash))]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum NgsiLdAttribute {
    Property(NgsiLdProperty),
    Relationship(NgsiLdRelationship),
    GeoProperty(NgsiLdGeoProperty),
    ListRelationship(NgsiLdListRelationship),
    LanguageProperty(NgsiLdLanguageProperty),
    VocabProperty(NgsiLdVocabProperty),
    ListProperty(NgsiLdListProperty),
    JsonProperty(NgsiLdJsonProperty),
}

impl NgsiLdAttribute {
    /// The instant this instance was observed, if it is a temporal instance.
    ///
    /// Every attribute kind carries an optional `observedAt` (NGSI-LD 4.8), so temporal
    /// aggregation reads it here to tell a time-stamped observation from a static value.
    #[must_use]
    pub(crate) const fn observed_at(&self) -> Option<DateTime<Utc>> {
        match self {
            NgsiLdAttribute::Property(p) => p.observed_at,
            NgsiLdAttribute::Relationship(r) => r.observed_at,
            NgsiLdAttribute::GeoProperty(g) => g.observed_at,
            NgsiLdAttribute::ListRelationship(lr) => lr.observed_at,
            NgsiLdAttribute::LanguageProperty(lp) => lp.observed_at,
            NgsiLdAttribute::VocabProperty(vp) => vp.observed_at,
            NgsiLdAttribute::ListProperty(lp) => lp.observed_at,
            NgsiLdAttribute::JsonProperty(jp) => jp.observed_at,
        }
    }
}

impl SerializeRepr for NgsiLdAttribute {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation) -> Result<S::Ok, S::Error> {
        match self {
            NgsiLdAttribute::Property(p) => p.serialize_repr(serializer, representation),
            NgsiLdAttribute::Relationship(r) => r.serialize_repr(serializer, representation),
            NgsiLdAttribute::GeoProperty(g) => g.serialize_repr(serializer, representation),
            NgsiLdAttribute::ListRelationship(lr) => lr.serialize_repr(serializer, representation),
            NgsiLdAttribute::LanguageProperty(lp) => lp.serialize_repr(serializer, representation),
            NgsiLdAttribute::VocabProperty(vp) => vp.serialize_repr(serializer, representation),
            NgsiLdAttribute::ListProperty(lp) => lp.serialize_repr(serializer, representation),
            NgsiLdAttribute::JsonProperty(jp) => jp.serialize_repr(serializer, representation),
        }
    }
}

/// An entity's attributes, in the mapping's declaration order.
///
/// The keys are attribute names a mapping declared (trusted configuration rather than
/// attacker-controlled input), and every attribute of every record builds and probes this map, so it
/// hashes with `foldhash` rather than the standard library's `SipHash`.
pub type Attributes = IndexMap<NameBuf, NgsiLdAttributeWrapper, RandomState>;

/// One attribute's nested sub-attributes (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2), in declaration
/// order.
///
/// Keyed and hashed like [`Attributes`], for the same reason: the names come from the mapping.
pub type NestedAttributes = IndexMap<NameBuf, Box<NgsiLdAttributeWrapper>, RandomState>;

#[cfg(test)]
mod tests {
    use crate::{
        entity::{
            NgsiLdEntity,
            attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper, property::NgsiLdProperty},
            name::NameBuf,
            representation::NgsiLdSerializable,
        },
        value::types::Value,
    };
    use cassiopeia_common::representation::NgsiLdRepresentation;
    use compact_str::CompactString;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use urn_rs::Urn;

    fn property(value: Value) -> NgsiLdAttribute {
        NgsiLdAttribute::Property(NgsiLdProperty::new(value))
    }

    fn entity_with(name: &str, wrapper: NgsiLdAttributeWrapper) -> NgsiLdEntity {
        let id: Urn = "urn:ngsi-ld:Sensor:1".parse().expect("valid urn");
        NgsiLdEntity::builder(id, NameBuf::new("Sensor").expect("valid name"))
            .attribute(NameBuf::new(name).expect("valid name"), wrapper)
            .build()
    }

    fn written(entity: &NgsiLdEntity, representation: NgsiLdRepresentation, name: &str) -> Option<JsonValue> {
        entity.to_json(representation).expect("entity serializes").get(name).cloned()
    }

    #[test]
    fn a_property_whose_value_is_an_empty_string_is_written_in_every_representation() {
        let entity = entity_with("label", NgsiLdAttributeWrapper::single(property(Value::String(CompactString::new("")))));

        assert_eq!(
            written(&entity, NgsiLdRepresentation::Normalized, "label"),
            Some(json!({"type": "Property", "value": ""}))
        );
        assert_eq!(written(&entity, NgsiLdRepresentation::Concise, "label"), Some(json!("")));
        assert_eq!(written(&entity, NgsiLdRepresentation::Simplified, "label"), Some(json!("")));
    }

    #[test]
    fn a_property_whose_value_is_an_empty_array_or_object_is_written() {
        let entity = entity_with("readings", NgsiLdAttributeWrapper::single(property(Value::Array(Vec::new()))));
        assert_eq!(written(&entity, NgsiLdRepresentation::Simplified, "readings"), Some(json!([])));

        let entity = entity_with("reading", NgsiLdAttributeWrapper::single(property(Value::Object(Box::default()))));
        assert_eq!(written(&entity, NgsiLdRepresentation::Simplified, "reading"), Some(json!({})));
    }

    #[test]
    fn every_instance_of_a_multi_instance_attribute_is_written() {
        let entity = entity_with(
            "temperature",
            NgsiLdAttributeWrapper::Multi(vec![property(Value::try_from(1.5).unwrap()), property(Value::String(CompactString::new("")))]),
        );

        assert_eq!(written(&entity, NgsiLdRepresentation::Concise, "temperature"), Some(json!([1.5, ""])));
    }
}
