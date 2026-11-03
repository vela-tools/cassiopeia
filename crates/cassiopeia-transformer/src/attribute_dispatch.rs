use crate::{
    attribute_builder::{build_geo_property, build_json_property, build_language_property, build_list_property, build_property, build_vocab_property},
    attribute_store::AttributeStore,
    metadata::transform_metadata,
    property_instances::build_multi_instance_attribute,
    qualifier_cache::QualifierCache,
    relationship_assembly::{
        build_list_relationship_attribute,
        build_multi_instance_list_relationship,
        build_multi_instance_relationship,
        build_single_relationship,
    },
};
use cassiopeia_ir::{attribute_content::AttributeContent, entity::AttributeValues, resolved_attribute::ResolvedAttribute};
use cassiopeia_mapping::attribute::Attribute;
use cassiopeia_ngsi_ld::{
    entity::{
        attribute::{NgsiLdAttributeKind, NgsiLdAttributeWrapper, language_property::LanguageMap},
        name::NameBuf,
    },
    value::{json_payload::JsonPayload, types::Value},
};
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
use iri_rs::IriBuf;

/// Builds one NGSI-LD attribute from its mapping declaration and the entity's resolved data.
///
/// A property draws its value from the value map, a relationship its objects from the relationship
/// map; consuming them here (`swap_remove`) leaves any value with no declaration behind. An
/// attribute whose value or objects are absent yields no attribute.
pub(crate) fn build_attribute(
    name: &NameBuf,
    config: &Attribute,
    store: &mut AttributeStore<'_>,
    cache: &mut QualifierCache<'_>,
    unreadable: &UnreadableTimestamps,
) -> Option<NgsiLdAttributeWrapper> {
    let AttributeStore {
        values,
        relationships,
        instance_relationships,
        metadata,
    } = store;
    let metadata = *metadata;
    let key = name.as_str();
    // Multi-attribute instances (ETSI GS CIM 009 v1.9.1 clause 4.5.5) are valid on every reified
    // attribute kind; each family reads its instances from the store populated for it: values for a
    // Property, per-instance objects for a Relationship or a `ListRelationship`.
    if config.instances().is_some() {
        return match config.kind() {
            NgsiLdAttributeKind::Property
            | NgsiLdAttributeKind::GeoProperty
            | NgsiLdAttributeKind::LanguageProperty
            | NgsiLdAttributeKind::VocabProperty
            | NgsiLdAttributeKind::ListProperty
            | NgsiLdAttributeKind::JsonProperty => build_multi_instance_attribute(name, config, values, metadata, cache, unreadable),
            NgsiLdAttributeKind::Relationship => build_multi_instance_relationship(name, config, instance_relationships, metadata, cache, unreadable),
            NgsiLdAttributeKind::ListRelationship => build_multi_instance_list_relationship(name, config, instance_relationships, metadata, cache, unreadable),
        };
    }
    match config.kind() {
        NgsiLdAttributeKind::Property => {
            let value = take_value(values, key)?;
            Some(build_property(value, transform_metadata(metadata, name, None, cache, unreadable)))
        }
        NgsiLdAttributeKind::GeoProperty => {
            // Extraction leaves a GeoProperty's value as the geometry it typed under the declaration's
            // own `geometry` policy, or no value at all, so there is nothing to parse here.
            let geometry = take_value(values, key)?.into_geometry()?;
            Some(build_geo_property(geometry, transform_metadata(metadata, name, None, cache, unreadable)))
        }
        NgsiLdAttributeKind::VocabProperty => {
            let iri = take_vocab(values, key)?;
            Some(build_vocab_property(iri, transform_metadata(metadata, name, None, cache, unreadable)))
        }
        NgsiLdAttributeKind::ListProperty => {
            let value = take_value(values, key)?;
            Some(build_list_property(value, transform_metadata(metadata, name, None, cache, unreadable)))
        }
        NgsiLdAttributeKind::JsonProperty => {
            let payload = take_json(values, key)?;
            Some(build_json_property(payload, transform_metadata(metadata, name, None, cache, unreadable)))
        }
        NgsiLdAttributeKind::LanguageProperty => {
            let language_map = take_language_map(values, key)?;
            Some(build_language_property(
                language_map,
                transform_metadata(metadata, name, None, cache, unreadable),
            ))
        }
        NgsiLdAttributeKind::Relationship => build_single_relationship(name, config, relationships, metadata, cache, unreadable),
        NgsiLdAttributeKind::ListRelationship => build_list_relationship_attribute(name, config, relationships, metadata, cache, unreadable),
    }
}

/// Takes the single content a single-instance declaration resolved to.
///
/// The extractor resolves a declaration without `instances` to a single content, so an instance
/// list has no single value to give and yields no attribute.
fn take_content(values: &mut AttributeValues, key: &str) -> Option<AttributeContent> {
    match values.swap_remove(key)? {
        ResolvedAttribute::Single(content) => Some(content),
        ResolvedAttribute::Instances(_) => None,
    }
}

/// Takes the NGSI-LD Value a single-instance Property-family declaration resolved to.
fn take_value(values: &mut AttributeValues, key: &str) -> Option<Value> {
    match take_content(values, key)? {
        AttributeContent::Value(value) => Some(value),
        AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_) => None,
    }
}

/// Takes the raw JSON a single-instance `JsonProperty` declaration resolved to.
fn take_json(values: &mut AttributeValues, key: &str) -> Option<JsonPayload> {
    match take_content(values, key)? {
        AttributeContent::Json(payload) => Some(payload),
        AttributeContent::Value(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_) => None,
    }
}

/// Takes the vocabulary IRI a single-instance `VocabProperty` declaration resolved to.
fn take_vocab(values: &mut AttributeValues, key: &str) -> Option<IriBuf> {
    match take_content(values, key)? {
        AttributeContent::Vocab(iri) => Some(iri),
        AttributeContent::Value(_) | AttributeContent::Json(_) | AttributeContent::Language(_) => None,
    }
}

/// Takes the `languageMap` a single-instance `LanguageProperty` declaration resolved to.
fn take_language_map(values: &mut AttributeValues, key: &str) -> Option<LanguageMap> {
    match take_content(values, key)? {
        AttributeContent::Language(language_map) => Some(language_map),
        AttributeContent::Value(_) | AttributeContent::Json(_) | AttributeContent::Vocab(_) => None,
    }
}
