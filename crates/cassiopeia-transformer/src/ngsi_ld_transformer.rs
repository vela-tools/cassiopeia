use crate::{
    attribute_builder::{
        build_geo_property,
        build_json_property,
        build_language_property,
        build_list_property,
        build_list_relationship,
        build_property,
        build_relationship,
        build_sub_attribute,
        build_vocab_property,
    },
    attribute_store::AttributeStore,
    error::Result,
    metadata::{MetadataSummary, transform_metadata},
    observed_at_cache::ObservedAtCache,
    qualifier_cache::QualifierCache,
    transformer::Transformer,
    unit_code_cache::UnitCodeCache,
};
use cassiopeia_common::parallelism::Parallelism;
use cassiopeia_ir::{
    entity::{AttributeValues, Entity},
    mapped::Mapped,
    metadata::EntityMetadata,
    relationships::{InstanceRelationships, Relationships},
};
use cassiopeia_mapping::attribute::Attribute;
use cassiopeia_ngsi_ld::{
    entity::{
        NgsiLdEntity,
        attribute::{NgsiLdAttribute, NgsiLdAttributeKind, NgsiLdAttributeWrapper, list_relationship::NgsiLdListRelationship},
        builder::{NgsiLdEntityBuilder, attribute::RelationshipBuilder},
        name::NameBuf,
    },
    value::types::Value,
};
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
use rayon::prelude::*;
use urn_rs::Urn;

/// The standard [`Transformer`]: assembles an [`NgsiLdEntity`] from an extracted entity.
#[derive(Debug, Clone, Default)]
pub struct NgsiLdTransformer {
    parallelism: Parallelism,
}

impl NgsiLdTransformer {
    /// Builds a transformer, transforming batches in parallel by default.
    #[must_use]
    pub const fn new() -> NgsiLdTransformer {
        NgsiLdTransformer {
            parallelism: Parallelism::Parallel,
        }
    }

    /// Chooses whether batches are transformed in parallel or sequentially.
    #[must_use]
    pub const fn with_parallelism(mut self, parallelism: Parallelism) -> NgsiLdTransformer {
        self.parallelism = parallelism;
        self
    }
}

/// Transforms one entity against a caller-owned unit-code cache.
///
/// Taking the cache as an argument is what lets a batch keep one per rayon worker: a `unitCode` is a
/// mapping literal drawn from a fixed set, so resolving it once per entity still paid a full
/// UN/CEFACT list match on the first attribute of every record. The result is identical either way.
fn transform_entity(mapped: Mapped<Entity>, unit_codes: &mut UnitCodeCache, unreadable: &UnreadableTimestamps) -> NgsiLdEntity {
    let (entity, mappings) = mapped.into_mappings();
    // Nested relationships were folded into `metadata` as sub-attributes by the extractor, so the
    // transformer reads them from there and ignores the raw nested-relationship map here.
    let (id, _data, scope, mut relationships, values, metadata, instance_relationships, _nested_relationships) = entity.into_parts();
    let mut values = values.unwrap_or_default();
    // Empty in the common case: `IndexMap::default()` allocates nothing until first insert, so an
    // entity with no instance list relationships pays no allocation here.
    let mut instance_relationships = instance_relationships.unwrap_or_default();

    // Every carried mapping targets the same id, so their data models agree on the entity type (it
    // is embedded in the URN); the first names it.
    let entity_type = mappings[0].data_model().entity_type().clone();
    let mut builder = NgsiLdEntityBuilder::new(id, entity_type);
    if let Some(scope) = scope {
        builder = builder.scope(scope);
    }

    // One `observedAt` memo per entity: a mapping repeats the same template across its
    // attributes, so each distinct text is parsed once per entity instead of once per attribute.
    // It cannot outlive the entity: an `observedAt` is record data, so a longer-lived linear-scan
    // memo would grow without bound. That is why the unit codes are memoised separately and
    // borrowed in beside it.
    let mut observed_at = ObservedAtCache::new();
    let mut cache = QualifierCache::new(&mut observed_at, unit_codes);

    let mut store = AttributeStore {
        values: &mut values,
        relationships: &mut relationships,
        instance_relationships: &mut instance_relationships,
        metadata: metadata.as_ref(),
    };

    // A joined entity carries one mapping per contributing fragment; each mapping's attributes read
    // their own value or objects from the shared maps, building every attribute exactly once.
    for mapping in &mappings {
        for (name, config) in mapping.attributes() {
            if let Some(wrapper) = build_attribute(name, config, &mut store, &mut cache, unreadable) {
                builder = builder.attribute(name.clone(), wrapper);
            }
        }
    }

    builder.build()
}

impl Transformer for NgsiLdTransformer {
    fn transform(&self, mapped: Mapped<Entity>, unreadable: &UnreadableTimestamps) -> Result<NgsiLdEntity> {
        Ok(transform_entity(mapped, &mut UnitCodeCache::new(), unreadable))
    }

    fn transform_batch(&self, entities: Vec<Mapped<Entity>>, unreadable: &UnreadableTimestamps) -> Vec<Result<NgsiLdEntity>> {
        match self.parallelism {
            // `map_init` hands each rayon worker one cache for the whole batch, so a run's distinct
            // unit codes are resolved once per worker rather than once per record.
            Parallelism::Parallel => entities
                .into_par_iter()
                .map_init(UnitCodeCache::new, |unit_codes, entity| Ok(transform_entity(entity, unit_codes, unreadable)))
                .collect(),
            Parallelism::Sequential => {
                let mut unit_codes = UnitCodeCache::new();
                entities
                    .into_iter()
                    .map(|entity| Ok(transform_entity(entity, &mut unit_codes, unreadable)))
                    .collect()
            }
        }
    }
}

/// Builds one NGSI-LD attribute from its mapping declaration and the entity's resolved data.
///
/// A property draws its value from the value map, a relationship its objects from the relationship
/// map; consuming them here (`swap_remove`) leaves any value with no declaration behind. An
/// attribute whose value or objects are absent yields no attribute.
fn build_attribute(
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
    // attribute kind; each family reads its instances from the store the resolve stage populated for
    // it: values for a Property, flat objects for a Relationship, regrouped object lists for a
    // `ListRelationship`.
    if config.instances().is_some() {
        return match config.kind() {
            NgsiLdAttributeKind::Property
            | NgsiLdAttributeKind::GeoProperty
            | NgsiLdAttributeKind::LanguageProperty
            | NgsiLdAttributeKind::VocabProperty
            | NgsiLdAttributeKind::ListProperty
            | NgsiLdAttributeKind::JsonProperty => build_multi_instance_attribute(name, config, values, metadata, cache, unreadable),
            NgsiLdAttributeKind::Relationship => build_multi_instance_relationship(name, config, relationships, metadata, cache, unreadable),
            NgsiLdAttributeKind::ListRelationship => build_multi_instance_list_relationship(name, config, instance_relationships, metadata, cache, unreadable),
        };
    }
    match config.kind() {
        NgsiLdAttributeKind::Property => {
            let value = values.swap_remove(key)?;
            Some(build_property(value, transform_metadata(metadata, name, None, cache, unreadable)))
        }
        NgsiLdAttributeKind::GeoProperty => {
            let value = values.swap_remove(key)?;
            build_geo_property(&value, transform_metadata(metadata, name, None, cache, unreadable))
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

/// Builds a Property-family attribute as several instances that differ by `datasetId`.
///
/// The resolved value is an array with one element per declared instance; each non-null element
/// becomes its own attribute instance carrying the per-instance metadata (its `datasetId` and the
/// shared qualifiers). A null element is a missing instance and contributes nothing, so a model that
/// reports no value for this record simply has no instance (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
/// When no instance survives, the attribute is omitted.
fn build_multi_instance_attribute(
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

/// The entity type a relationship's target points at, when the declaration names one.
fn object_type(config: &Attribute) -> Option<NameBuf> {
    config
        .target()
        .as_ref()
        .and_then(|target| NameBuf::new(target.entity().entity_type().as_str()).ok())
}

/// Builds a single-object Relationship from the first target URN, if any.
fn build_single_relationship(
    name: &NameBuf,
    config: &Attribute,
    relationships: &mut Relationships,
    metadata: Option<&EntityMetadata>,
    cache: &mut QualifierCache<'_>,
    unreadable: &UnreadableTimestamps,
) -> Option<NgsiLdAttributeWrapper> {
    let object = relationships.swap_remove(name.as_str())?.into_iter().next()?;

    Some(build_relationship(
        object,
        object_type(config),
        transform_metadata(metadata, name, None, cache, unreadable),
    ))
}

/// Builds a single `ListRelationship` over every target URN, carrying the attribute's shared
/// qualifiers and any sub-attributes qualifying the list as a whole (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.2.2).
///
/// Several `datasetId`-tagged list-relationship instances are declared with `instances` and built by
/// [`build_multi_instance_list_relationship`]; a plain `ListRelationship` therefore always resolves to
/// one instance here.
fn build_list_relationship_attribute(
    name: &NameBuf,
    config: &Attribute,
    relationships: &mut Relationships,
    metadata: Option<&EntityMetadata>,
    cache: &mut QualifierCache<'_>,
    unreadable: &UnreadableTimestamps,
) -> Option<NgsiLdAttributeWrapper> {
    let objects = relationships.swap_remove(name.as_str())?;
    if objects.is_empty() {
        return None;
    }

    Some(build_list_relationship(
        objects,
        object_type(config),
        transform_metadata(metadata, name, None, cache, unreadable),
    ))
}

/// Builds one `Relationship` per object URN, each carrying the per-index metadata recorded for it.
///
/// Shared by the per-item `ListRelationship` (one `Relationship` per target so its qualifiers travel
/// with it) and the multi-attribute `Relationship` (one `Relationship` per `datasetId`-tagged
/// instance, ETSI GS CIM 009 v1.9.1 clause 4.5.5): both are a sequence of single-object relationships
/// aligned with per-index metadata, so both mint their instances here.
fn build_relationship_instances(
    objects: Vec<Urn>,
    object_type: Option<&NameBuf>,
    name: &NameBuf,
    metadata: Option<&EntityMetadata>,
    cache: &mut QualifierCache<'_>,
    unreadable: &UnreadableTimestamps,
) -> Vec<NgsiLdAttribute> {
    objects
        .into_iter()
        .enumerate()
        .map(|(index, object)| {
            let meta = transform_metadata(metadata, name, Some(index), cache, unreadable);
            let mut builder = RelationshipBuilder::new(object);
            if let Some(object_type) = object_type {
                builder = builder.object_type(object_type.clone());
            }
            if let Some(observed_at) = meta.observed_at {
                builder = builder.observed_at(observed_at);
            }
            if let Some(dataset_id) = meta.dataset_id {
                builder = builder.dataset_id(dataset_id);
            }
            for (name, wrapper) in meta.custom_attributes {
                builder = builder.attribute(name, *wrapper);
            }
            NgsiLdAttribute::Relationship(builder.build())
        })
        .collect()
}

/// Builds a multi-attribute Relationship: one `Relationship` instance per `datasetId`, each with its
/// own object (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
///
/// The resolve stage minted one object per surviving instance and the extractor recorded one metadata
/// entry per surviving instance, dropped in lockstep, so object `i` and metadata `i` describe the
/// same instance. When no instance survived, the attribute is omitted.
fn build_multi_instance_relationship(
    name: &NameBuf,
    config: &Attribute,
    relationships: &mut Relationships,
    metadata: Option<&EntityMetadata>,
    cache: &mut QualifierCache<'_>,
    unreadable: &UnreadableTimestamps,
) -> Option<NgsiLdAttributeWrapper> {
    let objects = relationships.swap_remove(name.as_str())?;
    if objects.is_empty() {
        return None;
    }

    Some(NgsiLdAttributeWrapper::Multi(build_relationship_instances(
        objects,
        object_type(config).as_ref(),
        name,
        metadata,
        cache,
        unreadable,
    )))
}

/// Builds a multi-attribute `ListRelationship`: one `ListRelationship` instance per `datasetId`, each
/// with its own `objectList` (ETSI GS CIM 009 v1.9.1 clause 4.5.5, EXAMPLE 19).
///
/// The extractor regrouped the flat objects into one list per surviving instance and recorded one
/// metadata entry per surviving instance, dropped in lockstep, so group `i` and metadata `i` describe
/// the same instance. When no instance survived, the attribute is omitted.
fn build_multi_instance_list_relationship(
    name: &NameBuf,
    config: &Attribute,
    instance_relationships: &mut InstanceRelationships,
    metadata: Option<&EntityMetadata>,
    cache: &mut QualifierCache<'_>,
    unreadable: &UnreadableTimestamps,
) -> Option<NgsiLdAttributeWrapper> {
    let groups = instance_relationships.swap_remove(name.as_str())?;
    if groups.is_empty() {
        return None;
    }

    let object_type = object_type(config);
    let instances = groups
        .into_iter()
        .enumerate()
        .map(|(index, object_list)| {
            let meta = transform_metadata(metadata, name, Some(index), cache, unreadable);
            // The shared object type is cloned per instance; a struct literal keeps this an
            // initialization rather than a re-assignment over the `None` the constructor would set.
            NgsiLdAttribute::ListRelationship(NgsiLdListRelationship {
                object_list,
                object_type: object_type.clone(),
                observed_at: meta.observed_at,
                dataset_id: meta.dataset_id,
                attributes: meta.custom_attributes,
            })
        })
        .collect::<Vec<NgsiLdAttribute>>();

    Some(NgsiLdAttributeWrapper::Multi(instances))
}

#[cfg(test)]
mod tests {
    use crate::{ngsi_ld_transformer::NgsiLdTransformer, transformer::Transformer};
    use cassiopeia_common::parallelism::Parallelism;
    use cassiopeia_ir::{
        entity::{AttributeValues, Entity},
        mapped::Mapped,
        metadata::MetadataStorage,
        sub_attribute::{SubAttribute, SubAttributes},
    };
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use cassiopeia_ngsi_ld::{
        entity::{
            NgsiLdEntity,
            attribute::{
                NgsiLdAttribute,
                NgsiLdAttributeKind,
                NgsiLdAttributeWrapper,
                list_relationship::NgsiLdListRelationship,
                property::NgsiLdProperty,
                relationship::NgsiLdRelationship,
            },
            name::NameBuf,
        },
        value::types::{Number, Value},
    };
    use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
    use cefact_units::UnitCode;
    use chrono::{DateTime, TimeZone, Utc};
    use indexmap::IndexMap;
    use serde_json::json;
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    /// A per-instance metadata entry carrying only a `datasetId`.
    fn dataset_entry(dataset_id: &str) -> SubAttributes {
        SubAttributes::from_iter([(name("datasetId"), qualifier(json!(dataset_id)))])
    }

    /// Every instance's `datasetId`, in order, for a `Multi` of relationships.
    fn relationship_dataset_ids(instances: &[NgsiLdAttribute]) -> Vec<Option<Urn>> {
        instances
            .iter()
            .map(|attr| {
                let NgsiLdAttribute::Relationship(relationship) = attr else {
                    panic!("expected a relationship instance, got {attr:?}");
                };
                relationship.dataset_id.clone()
            })
            .collect()
    }

    fn mapping(document: &str) -> Arc<Mapping> {
        let mut runner = TemplateRunner::new();
        Arc::new(Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap())
    }

    fn urn(value: &str) -> Urn {
        value.parse::<Urn>().unwrap()
    }

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    fn values(pairs: Vec<(&str, Value)>) -> AttributeValues {
        pairs.into_iter().map(|(key, value)| (NameBuf::new(key).unwrap(), value)).collect()
    }

    fn qualifier(value: serde_json::Value) -> SubAttribute {
        SubAttribute::new(NgsiLdAttributeKind::Property, value, IndexMap::default())
    }

    #[test]
    fn the_entity_type_comes_from_the_mapping_data_model() {
        let mapping = mapping(r#"{ version: "v4", dataModel: "dataModel.Weather/WeatherObserved", identity: { entityName: "W-{{ id }}" }, attributes: {} }"#);
        let entity = Entity::new(urn("urn:ngsi-ld:WeatherObserved:1"), json!({}), None, IndexMap::default(), None);

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        assert_eq!(result.entity_type.as_str(), "WeatherObserved");
    }

    #[test]
    fn attributes_are_emitted_in_mapping_declaration_order() {
        // Attribute order is the mapping's declaration order, carried by the `IndexMap` the entity
        // holds; it is a property of the map type, not of whichever hasher backs it.
        let mapping = mapping(
            r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: {
                zone: { source: "{{ zone }}" },
                humidity: { source: "{{ humidity }}" },
                airTemperature: { source: "{{ air }}" },
            } }"#,
        );
        let mut entity = Entity::new(urn("urn:ngsi-ld:Sensor:1"), json!({}), None, IndexMap::default(), None);
        entity.set_values(Some(values(vec![
            ("zone", Value::String("north".into())),
            ("humidity", Value::Number(Number::Integer(51))),
            ("airTemperature", Value::Number(Number::Integer(19))),
        ])));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        assert_eq!(
            result.attributes.keys().map(NameBuf::as_str).collect::<Vec<&str>>(),
            ["zone", "humidity", "airTemperature"]
        );
    }

    #[test]
    fn a_property_carries_its_resolved_value() {
        let mapping = mapping(
            r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: { temperature: { source: "{{ t }}", transformation: "float" } } }"#,
        );
        let value_map = values(vec![("temperature", Value::Number(Number::Float(25.5)))]);
        let entity = Entity::new(urn("urn:ngsi-ld:Sensor:1"), json!({}), None, IndexMap::default(), Some(value_map));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Single(attr)) = result.attributes.get(&name("temperature")) else {
            panic!("expected a single property");
        };
        let NgsiLdAttribute::Property(property) = attr.as_ref() else {
            panic!("expected a property");
        };
        assert_eq!(property.value, Value::Number(Number::Float(25.5)));
    }

    #[test]
    fn a_value_without_a_declaration_is_dropped() {
        let mapping = mapping(r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: {} }"#);
        let value_map = values(vec![("stray", Value::String("x".into()))]);
        let entity = Entity::new(urn("urn:ngsi-ld:Sensor:1"), json!({}), None, IndexMap::default(), Some(value_map));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        assert!(result.attributes.is_empty());
    }

    #[test]
    fn a_relationship_points_at_its_object_and_carries_the_object_type() {
        let mapping = mapping(
            r#"{ version: "v4", dataModel: "Device", identity: { entityName: "D-{{ id }}" }, attributes: { controlledAsset: { type: "Relationship", target: { entity: "Building" } } } }"#,
        );
        let mut relationships = IndexMap::default();
        relationships.insert(name("controlledAsset"), vec![urn("urn:ngsi-ld:Building:9")]);
        let entity = Entity::new(urn("urn:ngsi-ld:Device:1"), json!({}), None, relationships, None);

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Single(attr)) = result.attributes.get(&name("controlledAsset")) else {
            panic!("expected a single relationship");
        };
        let NgsiLdAttribute::Relationship(relationship) = attr.as_ref() else {
            panic!("expected a relationship");
        };
        assert_eq!(relationship.object, urn("urn:ngsi-ld:Building:9"));
        assert_eq!(relationship.object_type, Some(name("Building")));
    }

    #[test]
    fn a_list_relationship_collects_every_object() {
        let mapping = mapping(
            r#"{ version: "v4", dataModel: "Device", identity: { entityName: "D-{{ id }}" }, attributes: { assets: { type: "ListRelationship", target: { entity: "Building" } } } }"#,
        );
        let mut relationships = IndexMap::default();
        relationships.insert(name("assets"), vec![urn("urn:ngsi-ld:Building:1"), urn("urn:ngsi-ld:Building:2")]);
        let entity = Entity::new(urn("urn:ngsi-ld:Device:1"), json!({}), None, relationships, None);

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Single(attr)) = result.attributes.get(&name("assets")) else {
            panic!("expected a single list relationship");
        };
        let NgsiLdAttribute::ListRelationship(relationship) = attr.as_ref() else {
            panic!("expected a list relationship");
        };
        assert_eq!(relationship.object_list.len(), 2);
        assert_eq!(relationship.object_type, Some(name("Building")));
    }

    #[test]
    fn a_shared_observed_at_qualifier_lands_on_the_property() {
        let mapping = mapping(
            r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: { temperature: { source: "{{ t }}", transformation: "float", properties: { observedAt: { source: "{{ ts }}" } } } } }"#,
        );
        let value_map = values(vec![("temperature", Value::Number(Number::Float(20.0)))]);
        let mut metadata = IndexMap::default();
        let mut shared = IndexMap::default();
        shared.insert(name("observedAt"), qualifier(json!("2026-04-03T22:00:20Z")));
        metadata.insert(name("temperature"), MetadataStorage::shared(shared));
        let mut entity = Entity::new(urn("urn:ngsi-ld:Sensor:1"), json!({}), None, IndexMap::default(), Some(value_map));
        entity.set_metadata(Some(metadata));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Single(attr)) = result.attributes.get(&name("temperature")) else {
            panic!("expected a single property");
        };
        let NgsiLdAttribute::Property(property) = attr.as_ref() else {
            panic!("expected a property");
        };
        assert!(property.observed_at.is_some());
    }

    /// The `observedAt` a temperature Property carries, or `None` when the attribute lost it.
    ///
    /// Asserting on the attribute alone is not enough: the attribute is emitted either way, and it
    /// is the sub-property that goes missing.
    fn observed_at_of(result: &NgsiLdEntity) -> Option<DateTime<Utc>> {
        let Some(NgsiLdAttributeWrapper::Single(attr)) = result.attributes.get(&name("temperature")) else {
            panic!("expected a single property");
        };
        let NgsiLdAttribute::Property(property) = attr.as_ref() else {
            panic!("expected a property");
        };
        property.observed_at
    }

    /// A sensor entity whose temperature carries `observed_at` as its `observedAt` qualifier.
    fn entity_observed_at(observed_at: &str) -> (Entity, Arc<Mapping>) {
        let mapping = mapping(
            r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: { temperature: { source: "{{ t }}", transformation: "float", properties: { observedAt: { source: "{{ ts }}" } } } } }"#,
        );
        let value_map = values(vec![("temperature", Value::Number(Number::Float(20.0)))]);
        let mut metadata = IndexMap::default();
        let mut shared = IndexMap::default();
        shared.insert(name("observedAt"), qualifier(json!(observed_at)));
        metadata.insert(name("temperature"), MetadataStorage::shared(shared));
        let mut entity = Entity::new(urn("urn:ngsi-ld:Sensor:1"), json!({}), None, IndexMap::default(), Some(value_map));
        entity.set_metadata(Some(metadata));

        (entity, mapping)
    }

    #[test]
    fn every_supported_observed_at_spelling_lands_on_the_property_as_the_same_instant() {
        let expected = Utc.with_ymd_and_hms(2026, 3, 1, 11, 4, 35).unwrap();

        for spelling in [
            "2026-03-01 11:04:35+00:00",
            "2026-03-01 11:04:35+0000",
            "2026-03-01T11:04:35+00:00",
            "2026-03-01T11:04:35Z",
            "2026-03-01 11:04:35",
            "2026-03-01 13:04:35+02:00",
        ] {
            let unreadable = UnreadableTimestamps::new();
            let (entity, mapping) = entity_observed_at(spelling);

            let result = NgsiLdTransformer::new().transform(Mapped::new(entity, mapping), &unreadable).unwrap();

            assert_eq!(observed_at_of(&result), Some(expected), "mismatch for {spelling:?}");
            assert!(unreadable.is_empty(), "{spelling:?} was recorded as unreadable");
        }
    }

    #[test]
    fn an_unreadable_observed_at_is_recorded_against_the_attribute_that_still_publishes() {
        let unreadable = UnreadableTimestamps::new();
        let (entity, mapping) = entity_observed_at("the third of March");

        let result = NgsiLdTransformer::new().transform(Mapped::new(entity, mapping), &unreadable).unwrap();

        // The attribute publishes without the qualifier, which is exactly why the loss has to be
        // named: nothing in the emitted entity says the qualifier was ever meant to be there.
        assert!(result.attributes.contains_key(&name("temperature")));
        assert_eq!(observed_at_of(&result), None);

        let entries = unreadable.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, name("temperature"));
        assert_eq!(entries[0].1.example.as_ref(), "the third of March");
    }

    #[test]
    fn a_blank_observed_at_is_an_absent_qualifier_rather_than_an_unreadable_one() {
        let unreadable = UnreadableTimestamps::new();
        let (entity, mapping) = entity_observed_at("   ");

        let result = NgsiLdTransformer::new().transform(Mapped::new(entity, mapping), &unreadable).unwrap();

        assert_eq!(observed_at_of(&result), None);
        assert!(unreadable.is_empty());
    }

    #[test]
    fn instances_become_one_property_per_dataset_id_and_null_instances_are_dropped() {
        let mapping = mapping(
            r#"{
                version: "v4",
                dataModel: "WeatherForecast",
                identity: { entityName: "F-{{ id }}" },
                attributes: {
                    temperature: {
                        type: "Property",
                        transformation: "float",
                        instances: [
                            { source: "{{ a }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:model:a" } } },
                            { source: "{{ b }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:model:b" } } },
                            { source: "{{ c }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:model:c" } } },
                        ],
                    },
                },
            }"#,
        );
        // The middle instance is null: a model that reported no value for this record, which must
        // leave no instance behind (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
        let value_map = values(vec![(
            "temperature",
            Value::Array(vec![Value::Number(Number::Float(1.0)), Value::Null, Value::Number(Number::Float(3.0))]),
        )]);
        let mut per_item = IndexMap::default();
        let entry = |urn: &str| SubAttributes::from_iter([(name("datasetId"), qualifier(json!(urn)))]);
        per_item.insert(
            name("temperature"),
            MetadataStorage::per_item(vec![
                entry("urn:ngsi-ld:dataset:model:a"),
                entry("urn:ngsi-ld:dataset:model:b"),
                entry("urn:ngsi-ld:dataset:model:c"),
            ]),
        );
        let mut entity = Entity::new(urn("urn:ngsi-ld:WeatherForecast:1"), json!({}), None, IndexMap::default(), Some(value_map));
        entity.set_metadata(Some(per_item));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Multi(instances)) = result.attributes.get(&name("temperature")) else {
            panic!("expected a multi-instance attribute");
        };
        assert_eq!(instances.len(), 2);
        let dataset_ids: Vec<&Urn> = instances
            .iter()
            .map(|attr| {
                let NgsiLdAttribute::Property(property) = attr else {
                    panic!("expected a property instance, got {attr:?}");
                };
                property.dataset_id.as_ref().expect("instance carries a datasetId")
            })
            .collect();
        assert_eq!(dataset_ids, vec![&urn("urn:ngsi-ld:dataset:model:a"), &urn("urn:ngsi-ld:dataset:model:c")]);
    }

    #[test]
    fn relationship_instances_become_a_multi_of_relationships_with_distinct_dataset_ids() {
        let mapping = mapping(
            r#"{
                version: "v4",
                dataModel: "dataModel.Aeronautics/Flight",
                identity: { entityName: "F-{{ id }}" },
                attributes: {
                    servesAirport: {
                        type: "Relationship",
                        target: { entity: "Airport" },
                        instances: [
                            { source: "{{ dep }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:departure" } } },
                            { source: "{{ arr }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:arrival" } } },
                        ],
                    },
                },
            }"#,
        );
        let mut relationships = IndexMap::default();
        relationships.insert(name("servesAirport"), vec![urn("urn:ngsi-ld:Airport:535"), urn("urn:ngsi-ld:Airport:340")]);
        let mut metadata = IndexMap::default();
        metadata.insert(
            name("servesAirport"),
            MetadataStorage::per_item(vec![
                dataset_entry("urn:ngsi-ld:dataset:role:departure"),
                dataset_entry("urn:ngsi-ld:dataset:role:arrival"),
            ]),
        );
        let mut entity = Entity::new(urn("urn:ngsi-ld:Flight:1"), json!({}), None, relationships, None);
        entity.set_metadata(Some(metadata));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Multi(instances)) = result.attributes.get(&name("servesAirport")) else {
            panic!("expected a multi-instance relationship");
        };
        assert_eq!(instances.len(), 2);
        let NgsiLdAttribute::Relationship(NgsiLdRelationship { object, object_type, .. }) = &instances[0] else {
            panic!("expected a relationship instance");
        };
        assert_eq!(*object, urn("urn:ngsi-ld:Airport:535"));
        assert_eq!(*object_type, Some(name("Airport")));
        assert_eq!(
            relationship_dataset_ids(instances),
            vec![Some(urn("urn:ngsi-ld:dataset:role:departure")), Some(urn("urn:ngsi-ld:dataset:role:arrival"))]
        );
    }

    #[test]
    fn a_dropped_relationship_instance_leaves_survivors_aligned_with_their_dataset_ids() {
        // Three instances were declared, but the resolve/extract stages compacted the middle one out,
        // so the transformer sees two objects and two metadata entries and zips them in order.
        let mapping = mapping(
            r#"{
                version: "v4",
                dataModel: "dataModel.Aeronautics/Flight",
                identity: { entityName: "F-{{ id }}" },
                attributes: {
                    servesAirport: {
                        type: "Relationship",
                        target: { entity: "Airport" },
                        instances: [
                            { source: "{{ a }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:departure" } } },
                            { source: "{{ b }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:via" } } },
                            { source: "{{ c }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:arrival" } } },
                        ],
                    },
                },
            }"#,
        );
        let mut relationships = IndexMap::default();
        relationships.insert(name("servesAirport"), vec![urn("urn:ngsi-ld:Airport:1"), urn("urn:ngsi-ld:Airport:3")]);
        let mut metadata = IndexMap::default();
        metadata.insert(
            name("servesAirport"),
            MetadataStorage::per_item(vec![
                dataset_entry("urn:ngsi-ld:dataset:role:departure"),
                dataset_entry("urn:ngsi-ld:dataset:role:arrival"),
            ]),
        );
        let mut entity = Entity::new(urn("urn:ngsi-ld:Flight:1"), json!({}), None, relationships, None);
        entity.set_metadata(Some(metadata));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Multi(instances)) = result.attributes.get(&name("servesAirport")) else {
            panic!("expected a multi-instance relationship");
        };
        assert_eq!(
            relationship_dataset_ids(instances),
            vec![Some(urn("urn:ngsi-ld:dataset:role:departure")), Some(urn("urn:ngsi-ld:dataset:role:arrival"))]
        );
    }

    #[test]
    fn list_relationship_instances_become_a_multi_of_list_relationships() {
        let mapping = mapping(
            r#"{
                version: "v4",
                dataModel: "Route",
                identity: { entityName: "R-{{ id }}" },
                attributes: {
                    servesAirports: {
                        type: "ListRelationship",
                        target: { entity: "Airport" },
                        instances: [
                            { source: "{{ a }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:departure" } } },
                            { source: "{{ b }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:arrival" } } },
                        ],
                    },
                },
            }"#,
        );
        let mut instance_relationships = IndexMap::default();
        instance_relationships.insert(
            name("servesAirports"),
            vec![
                vec![urn("urn:ngsi-ld:Airport:1"), urn("urn:ngsi-ld:Airport:2")],
                vec![urn("urn:ngsi-ld:Airport:3")],
            ],
        );
        let mut metadata = IndexMap::default();
        metadata.insert(
            name("servesAirports"),
            MetadataStorage::per_item(vec![
                dataset_entry("urn:ngsi-ld:dataset:role:departure"),
                dataset_entry("urn:ngsi-ld:dataset:role:arrival"),
            ]),
        );
        let mut entity = Entity::new(urn("urn:ngsi-ld:Route:1"), json!({}), None, IndexMap::default(), None);
        entity.set_metadata(Some(metadata));
        entity.set_instance_relationships(Some(instance_relationships));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Multi(instances)) = result.attributes.get(&name("servesAirports")) else {
            panic!("expected a multi-instance list relationship");
        };
        assert_eq!(instances.len(), 2);
        let lists: Vec<&NgsiLdListRelationship> = instances
            .iter()
            .map(|attr| {
                let NgsiLdAttribute::ListRelationship(list) = attr else {
                    panic!("expected a list relationship instance, got {attr:?}");
                };
                list
            })
            .collect();
        assert_eq!(lists[0].object_list, vec![urn("urn:ngsi-ld:Airport:1"), urn("urn:ngsi-ld:Airport:2")]);
        assert_eq!(lists[0].object_type, Some(name("Airport")));
        assert_eq!(lists[0].dataset_id, Some(urn("urn:ngsi-ld:dataset:role:departure")));
        assert_eq!(lists[1].object_list, vec![urn("urn:ngsi-ld:Airport:3")]);
        assert_eq!(lists[1].dataset_id, Some(urn("urn:ngsi-ld:dataset:role:arrival")));
    }

    #[test]
    fn geo_property_instances_become_a_multi_with_distinct_dataset_ids() {
        let mapping = mapping(
            r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: { location: { type: "GeoProperty", instances: [ { source: "{{ a }}" }, { source: "{{ b }}" } ] } } }"#,
        );
        let value_map = values(vec![(
            "location",
            Value::Array(vec![
                Value::from(json!({"type": "Point", "coordinates": [1.0, 2.0]})),
                Value::from(json!({"type": "Point", "coordinates": [3.0, 4.0]})),
            ]),
        )]);
        let mut metadata = IndexMap::default();
        metadata.insert(
            name("location"),
            MetadataStorage::per_item(vec![dataset_entry("urn:ngsi-ld:dataset:a"), dataset_entry("urn:ngsi-ld:dataset:b")]),
        );
        let mut entity = Entity::new(urn("urn:ngsi-ld:Sensor:1"), json!({}), None, IndexMap::default(), Some(value_map));
        entity.set_metadata(Some(metadata));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Multi(instances)) = result.attributes.get(&name("location")) else {
            panic!("expected a multi-instance geo property");
        };
        assert_eq!(instances.len(), 2);
        let dataset_ids: Vec<Option<&Urn>> = instances
            .iter()
            .map(|attr| {
                let NgsiLdAttribute::GeoProperty(geo) = attr else {
                    panic!("expected a geo property instance, got {attr:?}");
                };
                geo.dataset_id.as_ref()
            })
            .collect();
        assert_eq!(dataset_ids, vec![Some(&urn("urn:ngsi-ld:dataset:a")), Some(&urn("urn:ngsi-ld:dataset:b"))]);
    }

    #[test]
    fn a_rejected_vocab_property_instance_is_dropped_without_shifting_dataset_ids() {
        let mapping = mapping(
            r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: { category: { type: "VocabProperty", instances: [ { source: "{{ a }}" }, { source: "{{ b }}" }, { source: "{{ c }}" } ] } } }"#,
        );
        // The middle element is not a valid IRI, so its instance is dropped; the survivors keep their
        // own datasetIds because metadata is selected by original index (clause 4.5.5).
        let value_map = values(vec![(
            "category",
            Value::Array(vec![
                Value::String("https://example.org/a".into()),
                Value::String("not an iri".into()),
                Value::String("https://example.org/c".into()),
            ]),
        )]);
        let mut metadata = IndexMap::default();
        metadata.insert(
            name("category"),
            MetadataStorage::per_item(vec![
                dataset_entry("urn:ngsi-ld:dataset:a"),
                dataset_entry("urn:ngsi-ld:dataset:b"),
                dataset_entry("urn:ngsi-ld:dataset:c"),
            ]),
        );
        let mut entity = Entity::new(urn("urn:ngsi-ld:Sensor:1"), json!({}), None, IndexMap::default(), Some(value_map));
        entity.set_metadata(Some(metadata));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Multi(instances)) = result.attributes.get(&name("category")) else {
            panic!("expected a multi-instance vocab property");
        };
        assert_eq!(instances.len(), 2);
        let dataset_ids: Vec<Option<&Urn>> = instances
            .iter()
            .map(|attr| {
                let NgsiLdAttribute::VocabProperty(vocab) = attr else {
                    panic!("expected a vocab property instance, got {attr:?}");
                };
                vocab.dataset_id.as_ref()
            })
            .collect();
        assert_eq!(dataset_ids, vec![Some(&urn("urn:ngsi-ld:dataset:a")), Some(&urn("urn:ngsi-ld:dataset:c"))]);
    }

    #[test]
    fn language_list_and_json_property_instances_each_become_a_multi() {
        let cases = [
            (
                r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: { label: { type: "LanguageProperty", instances: [ { source: "{{ a }}" }, { source: "{{ b }}" } ] } } }"#,
                "label",
                Value::Array(vec![Value::from(json!({"en": "hello"})), Value::from(json!({"en": "world"}))]),
            ),
            (
                r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: { tags: { type: "ListProperty", instances: [ { source: "{{ a }}" }, { source: "{{ b }}" } ] } } }"#,
                "tags",
                Value::Array(vec![Value::from(json!(["x", "y"])), Value::from(json!(["z"]))]),
            ),
            (
                r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: { payload: { type: "JsonProperty", instances: [ { source: "{{ a }}" }, { source: "{{ b }}" } ] } } }"#,
                "payload",
                Value::Array(vec![Value::from(json!({"k": 1})), Value::from(json!({"k": 2}))]),
            ),
        ];

        for (document, key, array) in cases {
            let mapping = mapping(document);
            let value_map = values(vec![(key, array)]);
            let mut metadata = IndexMap::default();
            metadata.insert(
                name(key),
                MetadataStorage::per_item(vec![dataset_entry("urn:ngsi-ld:dataset:a"), dataset_entry("urn:ngsi-ld:dataset:b")]),
            );
            let mut entity = Entity::new(urn("urn:ngsi-ld:Sensor:1"), json!({}), None, IndexMap::default(), Some(value_map));
            entity.set_metadata(Some(metadata));

            let result = NgsiLdTransformer::new()
                .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
                .unwrap();

            let Some(NgsiLdAttributeWrapper::Multi(instances)) = result.attributes.get(&name(key)) else {
                panic!("expected a multi-instance attribute for {key}");
            };
            assert_eq!(instances.len(), 2, "expected two instances for {key}");
        }
    }

    #[test]
    fn a_relationship_carrying_a_nested_relationship_holds_it_in_its_attributes() {
        let mapping = mapping(
            r#"{ version: "v4", dataModel: "Movie", identity: { entityName: "M-{{ id }}" }, attributes: { hasLeadActor: { type: "Relationship", target: { entity: "Person" } } } }"#,
        );
        let mut relationships = IndexMap::default();
        relationships.insert(name("hasLeadActor"), vec![urn("urn:ngsi-ld:Person:31")]);
        let mut shared = IndexMap::default();
        shared.insert(
            name("playsCharacter"),
            SubAttribute::new_relationship(
                NgsiLdAttributeKind::Relationship,
                json!("urn:ngsi-ld:Character:JackSparrow"),
                Some(name("Character")),
                IndexMap::default(),
            ),
        );
        let mut metadata = IndexMap::default();
        metadata.insert(name("hasLeadActor"), MetadataStorage::shared(shared));
        let mut entity = Entity::new(urn("urn:ngsi-ld:Movie:1"), json!({}), None, relationships, None);
        entity.set_metadata(Some(metadata));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Single(attr)) = result.attributes.get(&name("hasLeadActor")) else {
            panic!("expected a single relationship");
        };
        let NgsiLdAttribute::Relationship(relationship) = attr.as_ref() else {
            panic!("expected a relationship");
        };
        let nested = relationship.attributes.get(&name("playsCharacter")).map(Box::as_ref);
        let Some(NgsiLdAttributeWrapper::Single(nested)) = nested else {
            panic!("expected a nested relationship sub-attribute");
        };
        let NgsiLdAttribute::Relationship(nested) = nested.as_ref() else {
            panic!("expected a nested relationship");
        };
        assert_eq!(nested.object.to_string(), "urn:ngsi-ld:Character:JackSparrow");
        assert_eq!(nested.object_type, Some(name("Character")));
    }

    #[test]
    fn a_list_relationship_carries_its_shared_sub_attributes() {
        let mapping = mapping(
            r#"{ version: "v4", dataModel: "Movie", identity: { entityName: "M-{{ id }}" }, attributes: { hasCast: { type: "ListRelationship", target: { entity: "Person" } } } }"#,
        );
        let mut relationships = IndexMap::default();
        relationships.insert(name("hasCast"), vec![urn("urn:ngsi-ld:Person:1"), urn("urn:ngsi-ld:Person:2")]);
        let mut shared = IndexMap::default();
        shared.insert(name("castSize"), qualifier(json!(2)));
        let mut metadata = IndexMap::default();
        metadata.insert(name("hasCast"), MetadataStorage::shared(shared));
        let mut entity = Entity::new(urn("urn:ngsi-ld:Movie:1"), json!({}), None, relationships, None);
        entity.set_metadata(Some(metadata));

        let result = NgsiLdTransformer::new()
            .transform(Mapped::new(entity, mapping), &UnreadableTimestamps::new())
            .unwrap();

        let Some(NgsiLdAttributeWrapper::Single(attr)) = result.attributes.get(&name("hasCast")) else {
            panic!("expected a single list relationship");
        };
        let NgsiLdAttribute::ListRelationship(list) = attr.as_ref() else {
            panic!("expected a list relationship");
        };
        assert_eq!(list.object_list.len(), 2);
        assert!(list.attributes.contains_key(&name("castSize")));
    }

    #[test]
    fn a_sequential_batch_transforms_every_entity() {
        let mapping = mapping(r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: {} }"#);
        let batch = vec![
            Mapped::new(
                Entity::new(urn("urn:ngsi-ld:Sensor:1"), json!({}), None, IndexMap::default(), None),
                Arc::clone(&mapping),
            ),
            Mapped::new(Entity::new(urn("urn:ngsi-ld:Sensor:2"), json!({}), None, IndexMap::default(), None), mapping),
        ];

        let results = NgsiLdTransformer::new()
            .with_parallelism(Parallelism::Sequential)
            .transform_batch(batch, &UnreadableTimestamps::new());

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(Result::is_ok));
    }

    #[test]
    fn a_parallel_batch_transforms_every_entity() {
        let mapping = mapping(r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: {} }"#);
        let batch = vec![
            Mapped::new(
                Entity::new(urn("urn:ngsi-ld:Sensor:1"), json!({}), None, IndexMap::default(), None),
                Arc::clone(&mapping),
            ),
            Mapped::new(Entity::new(urn("urn:ngsi-ld:Sensor:2"), json!({}), None, IndexMap::default(), None), mapping),
        ];

        let results = NgsiLdTransformer::new()
            .with_parallelism(Parallelism::Parallel)
            .transform_batch(batch, &UnreadableTimestamps::new());

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(Result::is_ok));
    }

    /// A sensor mapping whose temperature attribute declares both qualifiers.
    fn qualified_mapping() -> Arc<Mapping> {
        mapping(
            r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: { temperature: { source: "{{ t }}", transformation: "float", properties: { observedAt: { source: "{{ ts }}" }, unitCode: { source: "{{ u }}" } } } } }"#,
        )
    }

    /// A sensor entity whose temperature carries the given `observedAt` and `unitCode`.
    fn qualified_entity(id: &str, observed_at: &str, unit_code: &str) -> Entity {
        let value_map = values(vec![("temperature", Value::Number(Number::Float(20.0)))]);
        let mut shared = IndexMap::default();
        shared.insert(name("observedAt"), qualifier(json!(observed_at)));
        shared.insert(name("unitCode"), qualifier(json!(unit_code)));
        let mut metadata = IndexMap::default();
        metadata.insert(name("temperature"), MetadataStorage::shared(shared));
        let mut entity = Entity::new(urn(id), json!({}), None, IndexMap::default(), Some(value_map));
        entity.set_metadata(Some(metadata));
        entity
    }

    /// The temperature property of a transformed entity.
    fn temperature(entity: &NgsiLdEntity) -> &NgsiLdProperty {
        let Some(NgsiLdAttributeWrapper::Single(attr)) = entity.attributes.get(&name("temperature")) else {
            panic!("expected a single temperature property");
        };
        let NgsiLdAttribute::Property(property) = attr.as_ref() else {
            panic!("expected a property");
        };
        property
    }

    #[test]
    fn a_batch_sharing_one_unit_code_resolves_it_the_same_as_transforming_each_entity_alone() {
        let mapping = qualified_mapping();
        let batch = vec![
            Mapped::new(qualified_entity("urn:ngsi-ld:Sensor:1", "2026-04-03T22:00:20Z", "CEL"), Arc::clone(&mapping)),
            Mapped::new(qualified_entity("urn:ngsi-ld:Sensor:2", "2026-04-03T22:00:20Z", "CEL"), Arc::clone(&mapping)),
        ];
        let alone = NgsiLdTransformer::new()
            .transform(
                Mapped::new(qualified_entity("urn:ngsi-ld:Sensor:1", "2026-04-03T22:00:20Z", "CEL"), Arc::clone(&mapping)),
                &UnreadableTimestamps::new(),
            )
            .unwrap();

        let results = NgsiLdTransformer::new()
            .with_parallelism(Parallelism::Parallel)
            .transform_batch(batch, &UnreadableTimestamps::new());

        assert_eq!(results.len(), 2);
        for result in &results {
            let entity = result.as_ref().expect("transformed");
            assert_eq!(temperature(entity).unit_code, temperature(&alone).unit_code);
            assert_eq!(temperature(entity).unit_code, Some(UnitCode::Cel));
        }
    }

    #[test]
    fn a_batch_mixing_two_unit_codes_gives_each_entity_its_own() {
        let mapping = qualified_mapping();
        let batch = vec![
            Mapped::new(qualified_entity("urn:ngsi-ld:Sensor:1", "2026-04-03T22:00:20Z", "CEL"), Arc::clone(&mapping)),
            Mapped::new(qualified_entity("urn:ngsi-ld:Sensor:2", "2026-04-03T22:00:20Z", "KWH"), Arc::clone(&mapping)),
            Mapped::new(qualified_entity("urn:ngsi-ld:Sensor:3", "2026-04-03T22:00:20Z", "NOT-A-UNIT"), mapping),
        ];

        let results = NgsiLdTransformer::new()
            .with_parallelism(Parallelism::Sequential)
            .transform_batch(batch, &UnreadableTimestamps::new())
            .into_iter()
            .map(|result| temperature(&result.expect("transformed")).unit_code)
            .collect::<Vec<Option<UnitCode>>>();

        assert_eq!(results, vec![Some(UnitCode::Cel), Some(UnitCode::Kwh), None]);
    }

    #[test]
    fn two_entities_with_different_observed_at_values_each_parse_through_one_batch() {
        let mapping = qualified_mapping();
        let batch = vec![
            Mapped::new(qualified_entity("urn:ngsi-ld:Sensor:1", "2026-04-03T22:00:20Z", "CEL"), Arc::clone(&mapping)),
            Mapped::new(qualified_entity("urn:ngsi-ld:Sensor:2", "2026-04-03T23:15:00Z", "CEL"), mapping),
        ];

        let observed = NgsiLdTransformer::new()
            .with_parallelism(Parallelism::Sequential)
            .transform_batch(batch, &UnreadableTimestamps::new())
            .into_iter()
            .map(|result| temperature(&result.expect("transformed")).observed_at)
            .collect::<Vec<Option<DateTime<Utc>>>>();

        assert!(observed[0].is_some());
        assert!(observed[1].is_some());
        assert_ne!(observed[0], observed[1]);
    }
}
