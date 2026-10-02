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
use cassiopeia_mapping::attribute::Attribute;
use cassiopeia_ngsi_ld::entity::{
    attribute::{NgsiLdAttributeKind, NgsiLdAttributeWrapper},
    name::NameBuf,
};
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;

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
            let value = values.swap_remove(key)?;
            Some(build_property(value, transform_metadata(metadata, name, None, cache, unreadable)))
        }
        NgsiLdAttributeKind::GeoProperty => {
            // Extraction leaves a GeoProperty's value as the geometry it typed under the declaration's
            // own `geometry` policy, or no value at all, so there is nothing to parse here.
            let geometry = values.swap_remove(key)?.into_geometry()?;
            Some(build_geo_property(geometry, transform_metadata(metadata, name, None, cache, unreadable)))
        }
        NgsiLdAttributeKind::VocabProperty => {
            let value = values.swap_remove(key)?;
            build_vocab_property(&value, transform_metadata(metadata, name, None, cache, unreadable))
        }
        NgsiLdAttributeKind::ListProperty => {
            let value = values.swap_remove(key)?;
            Some(build_list_property(value, transform_metadata(metadata, name, None, cache, unreadable)))
        }
        NgsiLdAttributeKind::JsonProperty => {
            let value = values.swap_remove(key)?;
            Some(build_json_property(value, transform_metadata(metadata, name, None, cache, unreadable)))
        }
        NgsiLdAttributeKind::LanguageProperty => {
            let value = values.swap_remove(key)?;
            build_language_property(value, transform_metadata(metadata, name, None, cache, unreadable))
        }
        NgsiLdAttributeKind::Relationship => build_single_relationship(name, config, relationships, metadata, cache, unreadable),
        NgsiLdAttributeKind::ListRelationship => build_list_relationship_attribute(name, config, relationships, metadata, cache, unreadable),
    }
}
