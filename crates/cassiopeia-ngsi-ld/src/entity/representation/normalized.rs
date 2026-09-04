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
        DisplaySeq,
        DisplayStr,
        qualifiers::{serialize_dataset_id, serialize_instance_id, serialize_nested, serialize_object_type, serialize_observed_at, serialize_unit_code},
    },
};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use serde::{Serializer, ser::SerializeMap};

/// The representation these serializers emit; every nested attribute is emitted the same way.
const MODE: NgsiLdRepresentation = NgsiLdRepresentation::Normalized;

/// Serializes a Property in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_property<S: Serializer>(prop: &NgsiLdProperty, skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "Property")?;
    map.serialize_entry("value", &prop.value)?;
    serialize_observed_at(&mut map, prop.observed_at.as_ref())?;
    serialize_unit_code(&mut map, prop.unit_code.as_ref())?;
    serialize_dataset_id(&mut map, prop.dataset_id.as_ref())?;
    serialize_instance_id(&mut map, prop.instance_id.as_ref())?;
    serialize_nested(&mut map, &prop.attributes, MODE, skip_null)?;
    map.end()
}

/// Serializes a Relationship in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_relationship<S: Serializer>(rel: &NgsiLdRelationship, skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "Relationship")?;
    map.serialize_entry("object", &DisplayStr(&rel.object))?;
    serialize_object_type(&mut map, rel.object_type.as_ref())?;
    serialize_observed_at(&mut map, rel.observed_at.as_ref())?;
    serialize_dataset_id(&mut map, rel.dataset_id.as_ref())?;
    serialize_instance_id(&mut map, rel.instance_id.as_ref())?;
    serialize_nested(&mut map, &rel.attributes, MODE, skip_null)?;
    map.end()
}

/// Serializes a `GeoProperty` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_geo_property<S: Serializer>(geo: &NgsiLdGeoProperty, _skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "GeoProperty")?;
    map.serialize_entry("value", &geo.value)?;
    serialize_observed_at(&mut map, geo.observed_at.as_ref())?;
    serialize_dataset_id(&mut map, geo.dataset_id.as_ref())?;
    serialize_instance_id(&mut map, geo.instance_id.as_ref())?;
    map.end()
}

/// Serializes a `ListRelationship` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_list_relationship<S: Serializer>(rel: &NgsiLdListRelationship, skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "ListRelationship")?;
    map.serialize_entry("objectList", &DisplaySeq(&rel.object_list))?;
    serialize_object_type(&mut map, rel.object_type.as_ref())?;
    serialize_observed_at(&mut map, rel.observed_at.as_ref())?;
    serialize_dataset_id(&mut map, rel.dataset_id.as_ref())?;
    serialize_nested(&mut map, &rel.attributes, MODE, skip_null)?;
    map.end()
}

/// Serializes a `LanguageProperty` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_language_property<S: Serializer>(prop: &NgsiLdLanguageProperty, skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "LanguageProperty")?;
    map.serialize_entry("languageMap", &prop.language_map)?;
    serialize_observed_at(&mut map, prop.observed_at.as_ref())?;
    serialize_dataset_id(&mut map, prop.dataset_id.as_ref())?;
    serialize_nested(&mut map, &prop.attributes, MODE, skip_null)?;
    map.end()
}

/// Serializes a `VocabProperty` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_vocab_property<S: Serializer>(prop: &NgsiLdVocabProperty, skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "VocabProperty")?;
    map.serialize_entry("vocab", &DisplayStr(&prop.has_vocab))?;
    serialize_observed_at(&mut map, prop.observed_at.as_ref())?;
    serialize_dataset_id(&mut map, prop.dataset_id.as_ref())?;
    serialize_nested(&mut map, &prop.attributes, MODE, skip_null)?;
    map.end()
}

/// Serializes a `ListProperty` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_list_property<S: Serializer>(prop: &NgsiLdListProperty, skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "ListProperty")?;
    map.serialize_entry("valueList", &prop.has_value_list)?;
    serialize_observed_at(&mut map, prop.observed_at.as_ref())?;
    serialize_dataset_id(&mut map, prop.dataset_id.as_ref())?;
    serialize_nested(&mut map, &prop.attributes, MODE, skip_null)?;
    map.end()
}

/// Serializes a `JsonProperty` in the normalized representation.
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_json_property<S: Serializer>(prop: &NgsiLdJsonProperty, skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", "JsonProperty")?;
    map.serialize_entry("json", &prop.has_json)?;
    serialize_observed_at(&mut map, prop.observed_at.as_ref())?;
    serialize_dataset_id(&mut map, prop.dataset_id.as_ref())?;
    serialize_nested(&mut map, &prop.attributes, MODE, skip_null)?;
    map.end()
}
