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
    representation::{
        DisplayStr,
        qualifiers::{
            serialize_dataset_id,
            serialize_expires_at,
            serialize_instance_id,
            serialize_nested,
            serialize_object_type,
            serialize_observed_at,
            serialize_unit_code,
            serialize_value_type,
        },
    },
};
use cassiopeia_common::representation::NgsiLdRepresentation;
use serde::{Serialize, Serializer, ser::SerializeMap};
use urn_rs::Urn;

/// The representation these serializers emit; every nested attribute is emitted the same way.
const MODE: NgsiLdRepresentation = NgsiLdRepresentation::Normalized;

/// One element of a normalized `objectList`: a JSON object whose single member `object` holds one
/// target URI (ETSI GS CIM 009 v1.9.1 clause 4.5.22.2, table 5.2.37-1).
#[derive(Serialize)]
struct ListedObject<'a> {
    object: DisplayStr<&'a Urn>,
}

/// A normalized `objectList`: every target wrapped in a [`ListedObject`].
///
/// Clause 4.5.22.2 of ETSI GS CIM 009 v1.9.1 requires the object form here. A bare URI string is the
/// concise shape (clause 4.5.22.3). The NGSI-LD core `@context` declares `"@type": "@id"` on the
/// `object` term but not on `objectList`, so a bare string would expand to text rather than to a link.
struct ObjectList<'a>(&'a [Urn]);

// No derive maps each element into a `ListedObject`; streaming them through `collect_seq` avoids
// collecting an intermediate `Vec`.
impl Serialize for ObjectList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|object| ListedObject { object: DisplayStr(object) }))
    }
}

/// Serializes a Property in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_property<S: Serializer>(prop: &NgsiLdProperty, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "Property")?;
    map.serialize_entry("value", &prop.value)?;
    serialize_value_type(&mut map, prop.value_type.as_ref(), prop.value.temporal_type())?;
    serialize_observed_at(&mut map, prop.observed_at.as_ref())?;
    serialize_expires_at(&mut map, prop.expires_at.as_ref())?;
    serialize_unit_code(&mut map, prop.unit_code.as_ref())?;
    serialize_dataset_id(&mut map, prop.dataset_id.as_ref())?;
    serialize_instance_id(&mut map, prop.instance_id.as_ref())?;
    serialize_nested(&mut map, &prop.attributes, MODE)?;
    map.end()
}

/// Serializes a Relationship in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_relationship<S: Serializer>(rel: &NgsiLdRelationship, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "Relationship")?;
    map.serialize_entry("object", &DisplayStr(&rel.object))?;
    serialize_object_type(&mut map, rel.object_type.as_ref())?;
    serialize_observed_at(&mut map, rel.observed_at.as_ref())?;
    serialize_expires_at(&mut map, rel.expires_at.as_ref())?;
    serialize_dataset_id(&mut map, rel.dataset_id.as_ref())?;
    serialize_instance_id(&mut map, rel.instance_id.as_ref())?;
    serialize_nested(&mut map, &rel.attributes, MODE)?;
    map.end()
}

/// Serializes a `GeoProperty` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_geo_property<S: Serializer>(geo: &NgsiLdGeoProperty, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "GeoProperty")?;
    map.serialize_entry("value", &geo.value)?;
    serialize_value_type(&mut map, geo.value_type.as_ref(), None)?;
    serialize_observed_at(&mut map, geo.observed_at.as_ref())?;
    serialize_expires_at(&mut map, geo.expires_at.as_ref())?;
    serialize_dataset_id(&mut map, geo.dataset_id.as_ref())?;
    serialize_instance_id(&mut map, geo.instance_id.as_ref())?;
    map.end()
}

/// Serializes a `ListRelationship` in the normalized representation, each `objectList` element as a
/// `{ "object": URI }` object (ETSI GS CIM 009 v1.9.1 clause 4.5.22.2).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_list_relationship<S: Serializer>(rel: &NgsiLdListRelationship, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "ListRelationship")?;
    map.serialize_entry("objectList", &ObjectList(&rel.object_list))?;
    serialize_object_type(&mut map, rel.object_type.as_ref())?;
    serialize_observed_at(&mut map, rel.observed_at.as_ref())?;
    serialize_expires_at(&mut map, rel.expires_at.as_ref())?;
    serialize_dataset_id(&mut map, rel.dataset_id.as_ref())?;
    serialize_nested(&mut map, &rel.attributes, MODE)?;
    map.end()
}

/// Serializes a `LanguageProperty` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_language_property<S: Serializer>(prop: &NgsiLdLanguageProperty, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "LanguageProperty")?;
    map.serialize_entry("languageMap", &prop.language_map)?;
    serialize_value_type(&mut map, prop.value_type.as_ref(), None)?;
    serialize_observed_at(&mut map, prop.observed_at.as_ref())?;
    serialize_expires_at(&mut map, prop.expires_at.as_ref())?;
    serialize_dataset_id(&mut map, prop.dataset_id.as_ref())?;
    serialize_nested(&mut map, &prop.attributes, MODE)?;
    map.end()
}

/// Serializes a `VocabProperty` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_vocab_property<S: Serializer>(prop: &NgsiLdVocabProperty, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "VocabProperty")?;
    map.serialize_entry("vocab", &DisplayStr(&prop.has_vocab))?;
    serialize_value_type(&mut map, prop.value_type.as_ref(), None)?;
    serialize_observed_at(&mut map, prop.observed_at.as_ref())?;
    serialize_expires_at(&mut map, prop.expires_at.as_ref())?;
    serialize_dataset_id(&mut map, prop.dataset_id.as_ref())?;
    serialize_nested(&mut map, &prop.attributes, MODE)?;
    map.end()
}

/// Serializes a `ListProperty` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_list_property<S: Serializer>(prop: &NgsiLdListProperty, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "ListProperty")?;
    map.serialize_entry("valueList", &prop.has_value_list)?;
    serialize_value_type(&mut map, prop.value_type.as_ref(), None)?;
    serialize_observed_at(&mut map, prop.observed_at.as_ref())?;
    serialize_expires_at(&mut map, prop.expires_at.as_ref())?;
    serialize_dataset_id(&mut map, prop.dataset_id.as_ref())?;
    serialize_nested(&mut map, &prop.attributes, MODE)?;
    map.end()
}

/// Serializes a `JsonProperty` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_json_property<S: Serializer>(prop: &NgsiLdJsonProperty, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "JsonProperty")?;
    map.serialize_entry("json", &prop.has_json)?;
    serialize_value_type(&mut map, prop.value_type.as_ref(), None)?;
    serialize_observed_at(&mut map, prop.observed_at.as_ref())?;
    serialize_expires_at(&mut map, prop.expires_at.as_ref())?;
    serialize_dataset_id(&mut map, prop.dataset_id.as_ref())?;
    serialize_nested(&mut map, &prop.attributes, MODE)?;
    map.end()
}
