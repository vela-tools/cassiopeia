use crate::{
    attribute_builder::{build_list_relationship, build_relationship, list_relationship_instance, relationship_instance},
    metadata::transform_metadata,
    qualifier_cache::QualifierCache,
    qualifier_losses::QualifierLosses,
};
use cassiopeia_ir::{
    metadata::EntityMetadata,
    relationships::{InstanceRelationships, Relationships},
};
use cassiopeia_mapping::attribute::Attribute;
use cassiopeia_ngsi_ld::entity::{
    attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper},
    name::NameBuf,
};

/// The entity type a relationship's target points at, when the declaration names one.
fn object_type(config: &Attribute) -> Option<NameBuf> {
    config
        .target()
        .as_ref()
        .and_then(|target| NameBuf::new(target.entity().entity_type().as_str()).ok())
}

/// Builds a single-object Relationship from the first target URN, if any, and unless its `datasetId`
/// is no URI.
pub(crate) fn build_single_relationship(
    name: &NameBuf,
    config: &Attribute,
    relationships: &mut Relationships,
    metadata: Option<&EntityMetadata>,
    cache: &mut QualifierCache<'_>,
    losses: &QualifierLosses,
) -> Option<NgsiLdAttributeWrapper> {
    let object = relationships.swap_remove(name.as_str())?.into_iter().next()?;
    let meta = transform_metadata(metadata, name, None, cache, losses)?;

    Some(build_relationship(object, object_type(config), meta))
}

/// Builds a single `ListRelationship` over every target URN, carrying the attribute's shared
/// qualifiers and any sub-attributes qualifying the list as a whole (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.2.2), unless its `datasetId` is no URI.
///
/// Several `datasetId`-tagged list-relationship instances are declared with `instances` and built by
/// [`build_multi_instance_list_relationship`]; a plain `ListRelationship` therefore always resolves to
/// one instance here.
pub(crate) fn build_list_relationship_attribute(
    name: &NameBuf,
    config: &Attribute,
    relationships: &mut Relationships,
    metadata: Option<&EntityMetadata>,
    cache: &mut QualifierCache<'_>,
    losses: &QualifierLosses,
) -> Option<NgsiLdAttributeWrapper> {
    let objects = relationships.swap_remove(name.as_str())?;
    if objects.is_empty() {
        return None;
    }
    let meta = transform_metadata(metadata, name, None, cache, losses)?;

    Some(build_list_relationship(objects, object_type(config), meta))
}

/// Builds a multi-attribute Relationship: one `Relationship` instance per `datasetId`, each with its
/// own object (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
///
/// Each instance's object was minted under the instance's declaration index, and the extractor
/// recorded the instance's metadata at that same index, so an instance whose source named no target
/// is simply absent and every other instance still meets its own `datasetId`. A Relationship has a
/// single object, so an instance carrying several keeps the first, as a plain Relationship does. An
/// instance whose `datasetId` is no URI is dropped, since written without it, it would be the
/// default instance (clause 4.5.5.1). When no instance survives, the attribute is omitted.
pub(crate) fn build_multi_instance_relationship(
    name: &NameBuf,
    config: &Attribute,
    instance_relationships: &mut InstanceRelationships,
    metadata: Option<&EntityMetadata>,
    cache: &mut QualifierCache<'_>,
    losses: &QualifierLosses,
) -> Option<NgsiLdAttributeWrapper> {
    let per_instance = instance_relationships.swap_remove(name.as_str())?;
    let object_type = object_type(config);
    let instances = per_instance
        .into_iter()
        .filter_map(|(index, objects)| {
            let object = objects.into_iter().next()?;
            let meta = transform_metadata(metadata, name, Some(usize::from(index)), cache, losses)?;
            // The declaration names one object type for every instance, so each takes its own copy.
            Some(relationship_instance(object, object_type.clone(), meta))
        })
        .collect::<Vec<NgsiLdAttribute>>();

    if instances.is_empty() {
        None
    } else {
        Some(NgsiLdAttributeWrapper::Multi(instances))
    }
}

/// Builds a multi-attribute `ListRelationship`: one `ListRelationship` instance per `datasetId`, each
/// with its own `objectList` (ETSI GS CIM 009 v1.9.1 clause 4.5.5, EXAMPLE 19).
///
/// Each instance's objects were minted under the instance's declaration index, and the extractor
/// recorded the instance's metadata at that same index, so an instance that minted no object is
/// simply absent and every other instance still meets its own `datasetId`. An instance whose
/// `datasetId` is no URI is dropped, since written without it, it would be the default instance
/// (clause 4.5.5.1). When no instance survives, the attribute is omitted.
pub(crate) fn build_multi_instance_list_relationship(
    name: &NameBuf,
    config: &Attribute,
    instance_relationships: &mut InstanceRelationships,
    metadata: Option<&EntityMetadata>,
    cache: &mut QualifierCache<'_>,
    losses: &QualifierLosses,
) -> Option<NgsiLdAttributeWrapper> {
    let per_instance = instance_relationships.swap_remove(name.as_str())?;
    let object_type = object_type(config);
    let instances = per_instance
        .into_iter()
        .filter(|(_, object_list)| !object_list.is_empty())
        .filter_map(|(index, object_list)| {
            let meta = transform_metadata(metadata, name, Some(usize::from(index)), cache, losses)?;
            // The declaration names one object type for every instance, so each takes its own copy.
            Some(list_relationship_instance(object_list, object_type.clone(), meta))
        })
        .collect::<Vec<NgsiLdAttribute>>();

    if instances.is_empty() {
        None
    } else {
        Some(NgsiLdAttributeWrapper::Multi(instances))
    }
}
