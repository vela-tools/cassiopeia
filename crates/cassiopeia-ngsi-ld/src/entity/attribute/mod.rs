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
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
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

    /// Whether null-handling drops this occurrence entirely, so its key is omitted from the parent
    /// object. A [`NgsiLdAttributeWrapper::Multi`] is dropped only when every instance is null-like.
    #[must_use]
    pub(crate) fn is_skipped(&self, skip_null: NgsiLdSkipNull) -> bool {
        if skip_null != NgsiLdSkipNull::Skip {
            return false;
        }
        match self {
            NgsiLdAttributeWrapper::Single(attr) => attr.is_null_like(),
            NgsiLdAttributeWrapper::Multi(attrs) => attrs.iter().all(NgsiLdAttribute::is_null_like),
        }
    }
}

impl SerializeRepr for NgsiLdAttributeWrapper {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        match self {
            NgsiLdAttributeWrapper::Single(attr) => attr.serialize_repr(serializer, representation, skip_null),
            NgsiLdAttributeWrapper::Multi(attrs) => {
                let mut seq = serializer.serialize_seq(None)?;
                for attr in attrs {
                    if skip_null == NgsiLdSkipNull::Skip && attr.is_null_like() {
                        continue;
                    }
                    seq.serialize_element(&ReprAdapter::new(attr, representation, skip_null))?;
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
    /// Whether null-handling treats this instance as empty (a null or empty value / empty map), so
    /// skip-null omits it. A vocab property always carries a valid, non-empty IRI by construction;
    /// relationships reference an object and are never "empty"; a geo property carries one of the
    /// six geometry types clause 4.7 admits, which has no null or empty form: a value that carried
    /// no geometry never became a geo property in the first place.
    #[must_use]
    pub(crate) fn is_null_like(&self) -> bool {
        match self {
            NgsiLdAttribute::Property(p) => p.value.is_null() || p.value.is_empty_string(),
            NgsiLdAttribute::LanguageProperty(lp) => lp.language_map.is_empty(),
            NgsiLdAttribute::ListProperty(lp) => lp.has_value_list.is_empty(),
            NgsiLdAttribute::JsonProperty(jp) => jp.has_json.is_null() || jp.has_json.is_empty_string(),
            NgsiLdAttribute::GeoProperty(_) | NgsiLdAttribute::VocabProperty(_) | NgsiLdAttribute::Relationship(_) | NgsiLdAttribute::ListRelationship(_) => {
                false
            }
        }
    }

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
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        match self {
            NgsiLdAttribute::Property(p) => p.serialize_repr(serializer, representation, skip_null),
            NgsiLdAttribute::Relationship(r) => r.serialize_repr(serializer, representation, skip_null),
            NgsiLdAttribute::GeoProperty(g) => g.serialize_repr(serializer, representation, skip_null),
            NgsiLdAttribute::ListRelationship(lr) => lr.serialize_repr(serializer, representation, skip_null),
            NgsiLdAttribute::LanguageProperty(lp) => lp.serialize_repr(serializer, representation, skip_null),
            NgsiLdAttribute::VocabProperty(vp) => vp.serialize_repr(serializer, representation, skip_null),
            NgsiLdAttribute::ListProperty(lp) => lp.serialize_repr(serializer, representation, skip_null),
            NgsiLdAttribute::JsonProperty(jp) => jp.serialize_repr(serializer, representation, skip_null),
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
            attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper, geo_property::NgsiLdGeoProperty, property::NgsiLdProperty},
            name::NameBuf,
            representation::NgsiLdSerializable,
        },
        value::types::{Number, TemporalValue, Value},
    };
    use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use chrono::{TimeZone, Utc};
    use compact_str::CompactString;
    use urn_rs::Urn;

    fn property(value: Value) -> NgsiLdAttribute {
        NgsiLdAttribute::Property(NgsiLdProperty::new(value))
    }

    #[test]
    fn a_null_or_empty_string_property_is_null_like() {
        assert!(property(Value::Null).is_null_like());
        assert!(property(Value::String(CompactString::new(""))).is_null_like());
    }

    #[test]
    fn a_property_carrying_any_other_scalar_is_kept() {
        assert!(!property(Value::String(CompactString::new("x"))).is_null_like());
        assert!(!property(Value::Boolean(false)).is_null_like());
        assert!(!property(Value::Number(Number::Integer(0))).is_null_like());
        assert!(!property(Value::Number(Number::Float(0.0))).is_null_like());
        let instant = Utc.with_ymd_and_hms(2026, 4, 3, 22, 0, 20).unwrap();
        assert!(!property(Value::Temporal(TemporalValue::DateTime(instant))).is_null_like());
        let point = NgsiLdGeometry::Point {
            coordinates: [14.5, 46.05].into(),
        };
        assert!(!property(Value::Geospatial(Box::new(point))).is_null_like());
    }

    #[test]
    fn a_geo_property_is_always_kept_because_a_geometry_has_no_null_form() {
        let geo = NgsiLdAttribute::GeoProperty(NgsiLdGeoProperty::new(NgsiLdGeometry::Point {
            coordinates: [14.5, 46.05].into(),
        }));

        assert!(!geo.is_null_like());
    }

    #[test]
    fn an_empty_container_property_is_kept_rather_than_treated_as_null() {
        // An empty array and an empty object are values, not absences: skip-null must leave both in
        // place, the way an attribute carrying `[]` or `{}` has always been emitted.
        assert!(!property(Value::Array(Vec::new())).is_null_like());
        assert!(!property(Value::Object(Box::default())).is_null_like());
    }

    #[test]
    fn skip_null_serialization_keeps_a_property_whose_value_is_an_empty_array() {
        let id: Urn = "urn:ngsi-ld:Sensor:1".parse().expect("valid urn");
        let entity = NgsiLdEntity::builder(id, NameBuf::new("Sensor").expect("valid name"))
            .attribute(
                NameBuf::new("readings").expect("valid name"),
                NgsiLdAttributeWrapper::single(property(Value::Array(Vec::new()))),
            )
            .attribute(
                NameBuf::new("absent").expect("valid name"),
                NgsiLdAttributeWrapper::single(property(Value::Null)),
            )
            .build();

        let json = entity
            .to_json(NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip)
            .expect("entity serializes");

        assert_eq!(json.get("readings").and_then(|attr| attr.get("value")), Some(&serde_json::json!([])));
        assert!(json.get("absent").is_none());
    }
}
