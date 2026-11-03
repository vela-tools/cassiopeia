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
    representation::DisplaySeq,
};
use serde::{Serialize, Serializer, ser::SerializeMap};

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_property<S: Serializer>(prop: &NgsiLdProperty, serializer: S) -> Result<S::Ok, S::Error> {
    prop.value.serialize(serializer)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_relationship<S: Serializer>(rel: &NgsiLdRelationship, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(&rel.object)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_geo_property<S: Serializer>(geo: &NgsiLdGeoProperty, serializer: S) -> Result<S::Ok, S::Error> {
    geo.value.serialize(serializer)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_list_relationship<S: Serializer>(rel: &NgsiLdListRelationship, serializer: S) -> Result<S::Ok, S::Error> {
    DisplaySeq(&rel.object_list).serialize(serializer)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_language_property<S: Serializer>(prop: &NgsiLdLanguageProperty, serializer: S) -> Result<S::Ok, S::Error> {
    prop.language_map.serialize(serializer)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_vocab_property<S: Serializer>(prop: &NgsiLdVocabProperty, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(&prop.has_vocab)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_list_property<S: Serializer>(prop: &NgsiLdListProperty, serializer: S) -> Result<S::Ok, S::Error> {
    prop.has_value_list.serialize(serializer)
}

/// Serializes a `JsonProperty` in the simplified representation.
///
/// Clause 4.5.4 of ETSI GS CIM 009 v1.9.1 writes a `JsonProperty` as an object holding the single
/// member `json`, not as its bare payload, which would read as a Property's value and lose any null
/// inside it to JSON-LD expansion (clause 4.5.0).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_json_property<S: Serializer>(prop: &NgsiLdJsonProperty, serializer: S) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry("json", &prop.has_json)?;
    map.end()
}
