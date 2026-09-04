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
use cassiopeia_common::skip_null::NgsiLdSkipNull;
use serde::{Serialize, Serializer};

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_property<S: Serializer>(prop: &NgsiLdProperty, _skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    prop.value.serialize(serializer)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_relationship<S: Serializer>(rel: &NgsiLdRelationship, _skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(&rel.object)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_geo_property<S: Serializer>(geo: &NgsiLdGeoProperty, _skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    geo.value.serialize(serializer)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_list_relationship<S: Serializer>(rel: &NgsiLdListRelationship, _skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    DisplaySeq(&rel.object_list).serialize(serializer)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_language_property<S: Serializer>(prop: &NgsiLdLanguageProperty, _skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    prop.language_map.serialize(serializer)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_vocab_property<S: Serializer>(prop: &NgsiLdVocabProperty, _skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(&prop.has_vocab)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_list_property<S: Serializer>(prop: &NgsiLdListProperty, _skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    prop.has_value_list.serialize(serializer)
}

/// Serializes the attribute in the simplified representation (bare value).
///
/// # Errors
/// Returns the serializer's error if serialization fails.
pub fn serialize_json_property<S: Serializer>(prop: &NgsiLdJsonProperty, _skip_null: NgsiLdSkipNull, serializer: S) -> Result<S::Ok, S::Error> {
    prop.has_json.serialize(serializer)
}
