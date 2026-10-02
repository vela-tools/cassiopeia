use crate::{
    attribute_builder::build_sub_attribute,
    metadata::{MetadataSummary, transform_metadata},
    qualifier_cache::QualifierCache,
};
use cassiopeia_ir::{entity::AttributeValues, metadata::EntityMetadata};
use cassiopeia_mapping::attribute::Attribute;
use cassiopeia_ngsi_ld::{
    entity::{
        attribute::{NgsiLdAttribute, NgsiLdAttributeKind, NgsiLdAttributeWrapper},
        name::NameBuf,
    },
    value::types::Value,
};
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;

/// Builds a Property-family attribute as several instances that differ by `datasetId`.
///
/// The resolved value is an array with one element per declared instance; each non-null element
/// becomes its own attribute instance carrying the per-instance metadata (its `datasetId` and the
/// shared qualifiers). A null element is a missing instance and contributes nothing, so a model that
/// reports no value for this record simply has no instance (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
/// When no instance survives, the attribute is omitted.
pub(crate) fn build_multi_instance_attribute(
    name: &NameBuf,
    config: &Attribute,
    values: &mut AttributeValues,
    metadata: Option<&EntityMetadata>,
    cache: &mut QualifierCache<'_>,
    unreadable: &UnreadableTimestamps,
) -> Option<NgsiLdAttributeWrapper> {
    let items = match values.swap_remove(name.as_str())? {
        Value::Array(items) => items,
        value @ (Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Object(_)) => {
            vec![value]
        }
    };

    let instances = items
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            if item.is_null() {
                return None;
            }
            build_property_instance(*config.kind(), item, transform_metadata(metadata, name, Some(index), cache, unreadable))
        })
        .collect::<Vec<NgsiLdAttribute>>();

    if instances.is_empty() {
        None
    } else {
        Some(NgsiLdAttributeWrapper::Multi(instances))
    }
}

/// Builds one Property-family attribute instance from its value and metadata.
///
/// This is one instance of a multi-instance attribute, so it unwraps the single occurrence
/// [`build_sub_attribute`] produces for a Property-family kind. A relationship kind (which uses a
/// `ListRelationship` for multiple objects) or a builder that rejects the value (a non-IRI vocab
/// value, a non-object language value) yields none.
fn build_property_instance(kind: NgsiLdAttributeKind, value: Value, meta: MetadataSummary) -> Option<NgsiLdAttribute> {
    // A Property-family instance has no relationship target, so no object type is passed.
    match build_sub_attribute(kind, value, None, meta)? {
        NgsiLdAttributeWrapper::Single(attr) => Some(*attr),
        NgsiLdAttributeWrapper::Multi(_) => None,
    }
}
