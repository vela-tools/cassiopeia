use crate::{
    attribute::{resolution_context::ResolutionContext, resolver::resolve},
    dropped_geometries::DroppedGeometries,
    error::Result,
    extractor::Extractor,
};
use cassiopeia_common::parallelism::Parallelism;
use cassiopeia_ir::{
    assembled_entity::AssembledEntity,
    entity::{AttributeValues, Entity},
    mapped::{Mapped, Mappings},
    metadata::EntityMetadata,
    relationships::{InstanceRelationships, Relationships},
};
use cassiopeia_mapping::{mapping::Mapping, template::resolver::TemplateResolver};
use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
use indexmap::IndexMap;
use rayon::prelude::*;
use serde_json::Value as JsonValue;
use std::sync::Arc;
use urn_rs::Urn;

/// The standard [`Extractor`]: resolves each mapping attribute against the source record.
///
/// One shared [`TemplateResolver`] backs every extraction. It must have been taken from the runner
/// the mapping's templates were compiled with, so that every compiled template is visible.
pub struct EntityExtractor {
    resolver: TemplateResolver,
    parallelism: Parallelism,
}

impl EntityExtractor {
    /// Builds an extractor over a resolver, extracting batches in parallel by default.
    #[must_use]
    pub const fn new(resolver: TemplateResolver) -> EntityExtractor {
        EntityExtractor {
            resolver,
            parallelism: Parallelism::Parallel,
        }
    }

    /// Chooses whether batches are extracted in parallel or sequentially.
    #[must_use]
    pub const fn with_parallelism(mut self, parallelism: Parallelism) -> EntityExtractor {
        self.parallelism = parallelism;
        self
    }

    /// Reconstructs the per-instance object lists of every `ListRelationship` that carries instances.
    ///
    /// The expander emitted each such attribute's objects as flat child edges in instance-then-token
    /// order (ETSI GS CIM 009 v1.9.1 clause 4.5.5, EXAMPLE 19); assembly collected them into one flat
    /// `relationships[name]`. Here each instance's own token count re-derives its slice, so the flat
    /// list is regrouped into one `objectList` per instance without the relationship store ever
    /// recording instance boundaries. An instance that tokenizes to nothing was already dropped by the
    /// expander and contributes no slice, so the groups stay in lockstep with the per-instance
    /// metadata. Returns `None`, allocating no map, when the entity carries no instance list
    /// relationship.
    fn group_instance_relationships(&self, data: &JsonValue, mapping: &Mapping, relationships: &mut Relationships) -> Result<Option<InstanceRelationships>> {
        // First read each surviving instance's token count under an immutable borrow of the source
        // record, so the flat objects can then be drained under a mutable borrow without overlap.
        let mut counts_by_attribute: IndexMap<NameBuf, Vec<usize>> = IndexMap::default();
        for (name, config) in mapping.attributes() {
            if !matches!(config.kind(), NgsiLdAttributeKind::ListRelationship) {
                continue;
            }
            let Some(instances) = config.instances() else {
                continue;
            };

            let mut counts = Vec::new();
            for instance in instances {
                let count = match instance.compiled_source() {
                    Some(templates) => self.resolver.resolve_tokens(templates, data)?.len(),
                    None => 0,
                };
                if count > 0 {
                    counts.push(count);
                }
            }
            if !counts.is_empty() {
                counts_by_attribute.insert(name.clone(), counts);
            }
        }

        if counts_by_attribute.is_empty() {
            return Ok(None);
        }

        let mut grouped = InstanceRelationships::default();
        for (name, counts) in counts_by_attribute {
            // Draining removes the flat entry so the transformer reads the grouped form instead; the
            // objects are moved into their slices, never cloned.
            let mut objects = relationships.swap_remove(&name).unwrap_or_default().into_iter();
            let mut per_instance = Vec::with_capacity(counts.len());
            for count in counts {
                let group: Vec<Urn> = objects.by_ref().take(count).collect();
                if !group.is_empty() {
                    per_instance.push(group);
                }
            }
            if !per_instance.is_empty() {
                grouped.insert(name, per_instance);
            }
        }

        if grouped.is_empty() { Ok(None) } else { Ok(Some(grouped)) }
    }
}

impl Extractor for EntityExtractor {
    fn extract(&self, assembled: AssembledEntity, dropped: &DroppedGeometries, unreadable: &UnreadableTimestamps) -> Result<Mapped<Entity>> {
        let (id, scope, mut relationships, nested_relationships, fragments) = assembled.into_parts();
        let mut values = AttributeValues::default();
        let mut metadata = EntityMetadata::default();
        let mut instance_relationships = InstanceRelationships::default();
        let mappings: Mappings = fragments.iter().map(|(_, mapping)| Arc::clone(mapping)).collect();

        // Each fragment resolves through its own mapping against its own record; disjoint attribute
        // names across mappings make the union a concatenation, last-writer-wins on the rare collision.
        for (data, mapping) in &fragments {
            {
                let mut context = ResolutionContext::new(
                    data,
                    &self.resolver,
                    &relationships,
                    nested_relationships.as_ref(),
                    dropped,
                    unreadable,
                    Some(&mut metadata),
                );
                for (name, config) in mapping.attributes() {
                    let value = resolve(&mut context, name, config)?;
                    if !value.is_null() {
                        values.insert(name.clone(), value);
                    }
                }
            }
            if let Some(grouped) = self.group_instance_relationships(data, mapping, &mut relationships)? {
                for (name, groups) in grouped {
                    instance_relationships.insert(name, groups);
                }
            }
        }

        // The data is spent once every fragment is resolved; the transformer reads only the resolved
        // maps and the carried mappings, so the entity's own record is no longer needed.
        let mut entity = Entity::new(id, JsonValue::Null, scope, relationships, Some(values));
        if !metadata.is_empty() {
            entity.set_metadata(Some(metadata));
        }
        if !instance_relationships.is_empty() {
            entity.set_instance_relationships(Some(instance_relationships));
        }

        Ok(Mapped::with_mappings(entity, mappings))
    }

    fn extract_batch(&self, entities: Vec<AssembledEntity>, dropped: &DroppedGeometries, unreadable: &UnreadableTimestamps) -> Vec<Result<Mapped<Entity>>> {
        match self.parallelism {
            Parallelism::Parallel => entities.into_par_iter().map(|entity| self.extract(entity, dropped, unreadable)).collect(),
            Parallelism::Sequential => entities.into_iter().map(|entity| self.extract(entity, dropped, unreadable)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{dropped_geometries::DroppedGeometries, entity_extractor::EntityExtractor, extractor::Extractor};
    use cassiopeia_common::parallelism::Parallelism;
    use cassiopeia_expander::compiler::ExpanderCompiler;
    use cassiopeia_ir::{
        assembled_entity::AssembledEntity,
        entity::Entity,
        relationship_path::RelationshipPath,
        relationships::{NestedRelationships, Relationships},
        sub_attribute::SubAttribute,
    };
    use cassiopeia_mapping::{
        mapping::Mapping,
        template::{resolver::TemplateResolver, runner::TemplateRunner},
    };
    use cassiopeia_ngsi_ld::{
        entity::{attribute::NgsiLdAttributeKind, name::NameBuf},
        value::types::{Number, TemporalValue, Value},
    };
    use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;
    use chrono::{TimeZone, Utc};
    use serde_json::{Value as JsonValue, json};
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    /// Parses a mapping document and compiles its templates, returning both the shared resolver and
    /// the loaded mapping: the same preparation the pipeline performs before extraction.
    fn prepare(document: &str) -> (TemplateResolver, Arc<Mapping>) {
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, &mut runner);
        let resolver = runner.resolver();

        (resolver, Arc::new(mapping))
    }

    fn urn(value: &str) -> Urn {
        value.parse::<Urn>().unwrap()
    }

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn entity(id: &str, data: JsonValue) -> Entity {
        Entity::new(urn(id), data, None, Relationships::default(), None)
    }

    fn nested_path(segments: &[&str]) -> RelationshipPath {
        RelationshipPath::from_segments(segments.iter().map(|segment| name(segment)).collect())
    }

    #[test]
    fn a_float_attribute_is_resolved_and_typed() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "Sensor-{{ id }}" },
                attributes: { temperature: { source: "{{ temperature }}", transformation: "float" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:001", json!({"id": "001", "temperature": 25.5})), mapping);

        let (result, _) = extractor
            .extract(mapped, &DroppedGeometries::new(), &UnreadableTimestamps::new())
            .unwrap()
            .into_parts();
        let values = result.values().as_ref().expect("values set");

        assert_eq!(values.get(&name("temperature")), Some(&Value::Number(Number::Float(25.5))));
    }

    #[test]
    fn a_refused_geometry_drops_its_attribute_keeps_the_entity_and_records_the_refusal() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Zone",
                identity: { entityName: "Zone-{{ id }}" },
                attributes: {
                    reference: { source: "{{ id }}" },
                    location: { type: "GeoProperty", transformation: "polygon", source: "{{ geometry }}" },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let record = json!({
            "id": "001",
            "geometry": {
                "type": "MultiPolygon",
                "coordinates": [
                    [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                    [[[5.0, 5.0], [6.0, 5.0], [6.0, 6.0], [5.0, 5.0]]],
                ],
            },
        });
        let dropped = DroppedGeometries::new();

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Zone:001", record), mapping);
        let (result, _) = extractor.extract(mapped, &dropped, &UnreadableTimestamps::new()).unwrap().into_parts();
        let values = result.values().as_ref().expect("values set");

        // The unconvertible attribute is gone, but every other attribute of the entity survives.
        assert!(!values.contains_key(&name("location")));
        assert!(values.contains_key(&name("reference")));

        let entries = dropped.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.attribute, name("location"));
        assert_eq!(entries[0].1, 1);
    }

    #[test]
    fn every_supported_timestamp_spelling_resolves_to_the_same_instant() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "Sensor-{{ id }}" },
                attributes: { reading: { source: "{{ ts }}", transformation: "datetime" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let unreadable = UnreadableTimestamps::new();

        // Spellings a source may use for the same instant, including two with a UTC offset on a
        // space-separated (non-`T`) timestamp.
        for spelling in [
            "2026-03-01 11:04:35+00:00",
            "2026-03-01 11:04:35+0000",
            "2026-03-01T11:04:35+00:00",
            "2026-03-01T11:04:35Z",
            "2026-03-01 11:04:35",
            "2026-03-01 13:04:35+02:00",
        ] {
            let record = json!({"id": "001", "ts": spelling});
            let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:001", record), Arc::clone(&mapping));
            let (result, _) = extractor.extract(mapped, &DroppedGeometries::new(), &unreadable).unwrap().into_parts();
            let values = result.values().as_ref().expect("values set").clone();

            let expected = TemporalValue::DateTime(Utc.with_ymd_and_hms(2026, 3, 1, 11, 4, 35).unwrap());
            assert_eq!(values.get(&name("reading")), Some(&Value::Temporal(expected)), "mismatch for {spelling:?}");
        }

        assert!(unreadable.is_empty());
    }

    #[test]
    fn an_unreadable_timestamp_drops_its_attribute_keeps_the_entity_and_records_the_text() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "Sensor-{{ id }}" },
                attributes: {
                    reference: { source: "{{ id }}" },
                    reading: { source: "{{ ts }}", transformation: "datetime" },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let unreadable = UnreadableTimestamps::new();
        let record = json!({"id": "001", "ts": "the third of March"});

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:001", record), mapping);
        let (result, _) = extractor.extract(mapped, &DroppedGeometries::new(), &unreadable).unwrap().into_parts();
        let values = result.values().as_ref().expect("values set");

        // The unreadable attribute is gone, but every other attribute of the entity survives.
        assert!(!values.contains_key(&name("reading")));
        assert!(values.contains_key(&name("reference")));

        let entries = unreadable.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, name("reading"));
        assert_eq!(entries[0].1.example.as_ref(), "the third of March");
        assert_eq!(entries[0].1.occurrences.get(), 1);
    }

    #[test]
    fn a_blank_timestamp_is_an_absent_value_rather_than_an_unreadable_one() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "Sensor-{{ id }}" },
                attributes: { reading: { source: "{{ ts }}", transformation: "datetime" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let unreadable = UnreadableTimestamps::new();

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:001", json!({"id": "001", "ts": ""})), mapping);
        let (result, _) = extractor.extract(mapped, &DroppedGeometries::new(), &unreadable).unwrap().into_parts();

        assert!(!result.values().as_ref().expect("values set").contains_key(&name("reading")));
        assert!(unreadable.is_empty());
    }

    #[test]
    fn a_declared_conversion_keeps_the_same_geometry_attribute() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Zone",
                identity: { entityName: "Zone-{{ id }}" },
                attributes: {
                    location: {
                        type: "GeoProperty",
                        transformation: "polygon",
                        geometry: { convert: "largest" },
                        source: "{{ geometry }}",
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let record = json!({
            "id": "001",
            "geometry": {
                "type": "MultiPolygon",
                "coordinates": [
                    [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                    [[[5.0, 5.0], [8.0, 5.0], [8.0, 8.0], [5.0, 5.0]]],
                ],
            },
        });
        let dropped = DroppedGeometries::new();

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Zone:001", record), mapping);
        let (result, _) = extractor.extract(mapped, &dropped, &UnreadableTimestamps::new()).unwrap().into_parts();
        let values = result.values().as_ref().expect("values set");

        assert!(values.contains_key(&name("location")));
        assert!(dropped.is_empty());
    }

    #[test]
    fn an_attribute_resolving_to_null_is_omitted() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "Sensor-{{ id }}" },
                attributes: { humidity: { source: "{{ missing }}", transformation: "float" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:001", json!({"id": "001"})), mapping);

        let (result, _) = extractor
            .extract(mapped, &DroppedGeometries::new(), &UnreadableTimestamps::new())
            .unwrap()
            .into_parts();
        let values = result.values().as_ref().expect("values set");

        assert!(!values.contains_key(&name("humidity")));
    }

    #[test]
    fn attribute_properties_are_collected_as_metadata() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "Sensor-{{ id }}" },
                attributes: {
                    temperature: {
                        source: "{{ temperature }}",
                        transformation: "float",
                        properties: { unitCode: { source: "CEL" } },
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:001", json!({"id": "001", "temperature": 20.0})), mapping);

        let (result, _) = extractor
            .extract(mapped, &DroppedGeometries::new(), &UnreadableTimestamps::new())
            .unwrap()
            .into_parts();
        let metadata = result.metadata().as_ref().expect("metadata set");
        let shared = metadata
            .get(&name("temperature"))
            .expect("temperature metadata")
            .as_shared()
            .expect("shared metadata");

        assert_eq!(shared.get(&name("unitCode")).map(SubAttribute::value), Some(&json!("CEL")));
    }

    #[test]
    fn a_typed_sub_attribute_keeps_its_declared_kind() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Product",
                identity: { entityName: "Product-{{ id }}" },
                attributes: {
                    sugars: {
                        source: "{{ sugars }}",
                        transformation: "float",
                        properties: {
                            level: { source: "https://example.org/level/{{ level }}", type: "VocabProperty" },
                        },
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Product:1", json!({"id": "1", "sugars": 12.0, "level": "high"})), mapping);

        let (result, _) = extractor
            .extract(mapped, &DroppedGeometries::new(), &UnreadableTimestamps::new())
            .unwrap()
            .into_parts();
        let metadata = result.metadata().as_ref().expect("metadata set");
        let shared = metadata.get(&name("sugars")).expect("sugars metadata").as_shared().expect("shared metadata");
        let level = shared.get(&name("level")).expect("level sub-attribute");

        assert_eq!(*level.kind(), NgsiLdAttributeKind::VocabProperty);
        assert_eq!(level.value(), &json!("https://example.org/level/high"));
    }

    #[test]
    fn a_doubly_nested_sub_attribute_survives() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Product",
                identity: { entityName: "Product-{{ id }}" },
                attributes: {
                    sugars: {
                        source: "{{ sugars }}",
                        transformation: "float",
                        properties: {
                            level: {
                                source: "https://example.org/level/{{ level }}",
                                type: "VocabProperty",
                                properties: {
                                    source: { source: "measured" },
                                },
                            },
                        },
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Product:1", json!({"id": "1", "sugars": 12.0, "level": "high"})), mapping);

        let (result, _) = extractor
            .extract(mapped, &DroppedGeometries::new(), &UnreadableTimestamps::new())
            .unwrap()
            .into_parts();
        let metadata = result.metadata().as_ref().expect("metadata set");
        let shared = metadata.get(&name("sugars")).expect("sugars metadata").as_shared().expect("shared metadata");
        let level = shared.get(&name("level")).expect("level sub-attribute");
        let inner = level.metadata().get(&name("source")).expect("nested sub-attribute");

        assert_eq!(*level.kind(), NgsiLdAttributeKind::VocabProperty);
        assert_eq!(inner.value(), &json!("measured"));
    }

    #[test]
    fn instances_resolve_to_a_value_array_and_per_instance_metadata() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "WeatherForecast",
                identity: { entityName: "Forecast-{{ id }}" },
                attributes: {
                    temperatureMax: {
                        type: "Property",
                        transformation: "float",
                        properties: { unitCode: { source: "CEL" } },
                        instances: [
                            { source: "{{ a }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:model:a" } } },
                            { source: "{{ b }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:model:b" } } },
                        ],
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:WeatherForecast:1", json!({"id": "1", "a": 10.0, "b": 12.0})), mapping);

        let (result, _) = extractor
            .extract(mapped, &DroppedGeometries::new(), &UnreadableTimestamps::new())
            .unwrap()
            .into_parts();
        let values = result.values().as_ref().expect("values set");
        let metadata = result.metadata().as_ref().expect("metadata set");

        match values.get(&name("temperatureMax")) {
            Some(Value::Array(items)) => {
                assert_eq!(items, &vec![Value::Number(Number::Float(10.0)), Value::Number(Number::Float(12.0))]);
            }
            other => panic!("expected an array value, got {other:?}"),
        }

        let per_item = metadata
            .get(&name("temperatureMax"))
            .expect("temperatureMax metadata")
            .as_per_item()
            .expect("per-item metadata");
        assert_eq!(per_item.len(), 2);
        // Each instance carries the shared unitCode plus its own datasetId.
        assert_eq!(per_item[0].get(&name("unitCode")).map(SubAttribute::value), Some(&json!("CEL")));
        assert_eq!(
            per_item[0].get(&name("datasetId")).map(SubAttribute::value),
            Some(&json!("urn:ngsi-ld:dataset:model:a"))
        );
        assert_eq!(
            per_item[1].get(&name("datasetId")).map(SubAttribute::value),
            Some(&json!("urn:ngsi-ld:dataset:model:b"))
        );
    }

    #[test]
    fn relationship_instances_record_per_item_dataset_ids_and_no_value() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Flight",
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
        // The resolve stage already minted one object per instance; the extractor records the
        // per-instance metadata that aligns with them.
        let mut relationships = Relationships::default();
        relationships.insert(name("servesAirport"), vec![urn("urn:ngsi-ld:Airport:535"), urn("urn:ngsi-ld:Airport:340")]);
        let entity = Entity::new(
            urn("urn:ngsi-ld:Flight:1"),
            json!({"id": "1", "dep": "535", "arr": "340"}),
            None,
            relationships,
            None,
        );

        let (result, _) = EntityExtractor::new(resolver)
            .extract(
                AssembledEntity::from_single(entity, mapping),
                &DroppedGeometries::new(),
                &UnreadableTimestamps::new(),
            )
            .unwrap()
            .into_parts();

        assert!(!result.values().as_ref().unwrap().contains_key(&name("servesAirport")));
        let per_item = result.metadata().as_ref().unwrap().get(&name("servesAirport")).unwrap().as_per_item().unwrap();
        assert_eq!(per_item.len(), 2);
        assert_eq!(
            per_item[0].get(&name("datasetId")).map(SubAttribute::value),
            Some(&json!("urn:ngsi-ld:dataset:role:departure"))
        );
        assert_eq!(
            per_item[1].get(&name("datasetId")).map(SubAttribute::value),
            Some(&json!("urn:ngsi-ld:dataset:role:arrival"))
        );
    }

    #[test]
    fn a_dropped_relationship_instance_keeps_metadata_aligned() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Flight",
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
        // The middle instance's key `b` is absent, so the expander dropped it: only two objects, and
        // the extractor drops its metadata in lockstep.
        let mut relationships = Relationships::default();
        relationships.insert(name("servesAirport"), vec![urn("urn:ngsi-ld:Airport:1"), urn("urn:ngsi-ld:Airport:3")]);
        let entity = Entity::new(urn("urn:ngsi-ld:Flight:1"), json!({"id": "1", "a": "1", "c": "3"}), None, relationships, None);

        let (result, _) = EntityExtractor::new(resolver)
            .extract(
                AssembledEntity::from_single(entity, mapping),
                &DroppedGeometries::new(),
                &UnreadableTimestamps::new(),
            )
            .unwrap()
            .into_parts();

        let per_item = result.metadata().as_ref().unwrap().get(&name("servesAirport")).unwrap().as_per_item().unwrap();
        assert_eq!(per_item.len(), 2);
        assert_eq!(
            per_item[0].get(&name("datasetId")).map(SubAttribute::value),
            Some(&json!("urn:ngsi-ld:dataset:role:departure"))
        );
        assert_eq!(
            per_item[1].get(&name("datasetId")).map(SubAttribute::value),
            Some(&json!("urn:ngsi-ld:dataset:role:arrival"))
        );
    }

    #[test]
    fn list_relationship_instances_regroup_objects_and_compact_dataset_ids() {
        let (resolver, mapping) = prepare(
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
        // Flat objects in instance-then-token order: first instance's two tokens, then the second's one.
        let mut relationships = Relationships::default();
        relationships.insert(
            name("servesAirports"),
            vec![urn("urn:ngsi-ld:Airport:1"), urn("urn:ngsi-ld:Airport:2"), urn("urn:ngsi-ld:Airport:3")],
        );
        let entity = Entity::new(urn("urn:ngsi-ld:Route:1"), json!({"id": "1", "a": "1 2", "b": "3"}), None, relationships, None);

        let (result, _) = EntityExtractor::new(resolver)
            .extract(
                AssembledEntity::from_single(entity, mapping),
                &DroppedGeometries::new(),
                &UnreadableTimestamps::new(),
            )
            .unwrap()
            .into_parts();

        let groups = result.instance_relationships().as_ref().unwrap().get(&name("servesAirports")).unwrap();
        assert_eq!(
            groups,
            &vec![
                vec![urn("urn:ngsi-ld:Airport:1"), urn("urn:ngsi-ld:Airport:2")],
                vec![urn("urn:ngsi-ld:Airport:3")],
            ]
        );
        // The flat entry is drained so the transformer reads the grouped form.
        assert!(!result.relationships().contains_key(&name("servesAirports")));
        let per_item = result.metadata().as_ref().unwrap().get(&name("servesAirports")).unwrap().as_per_item().unwrap();
        assert_eq!(per_item.len(), 2);
        assert_eq!(
            per_item[0].get(&name("datasetId")).map(SubAttribute::value),
            Some(&json!("urn:ngsi-ld:dataset:role:departure"))
        );
        assert_eq!(
            per_item[1].get(&name("datasetId")).map(SubAttribute::value),
            Some(&json!("urn:ngsi-ld:dataset:role:arrival"))
        );
    }

    #[test]
    fn a_dropped_list_relationship_instance_keeps_groups_and_metadata_aligned() {
        let (resolver, mapping) = prepare(
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
                            { source: "{{ b }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:via" } } },
                            { source: "{{ c }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:arrival" } } },
                        ],
                    },
                },
            }"#,
        );
        // The middle instance tokenizes to nothing (`b` absent); its group and metadata both drop.
        let mut relationships = Relationships::default();
        relationships.insert(name("servesAirports"), vec![urn("urn:ngsi-ld:Airport:1"), urn("urn:ngsi-ld:Airport:3")]);
        let entity = Entity::new(urn("urn:ngsi-ld:Route:1"), json!({"id": "1", "a": "1", "c": "3"}), None, relationships, None);

        let (result, _) = EntityExtractor::new(resolver)
            .extract(
                AssembledEntity::from_single(entity, mapping),
                &DroppedGeometries::new(),
                &UnreadableTimestamps::new(),
            )
            .unwrap()
            .into_parts();

        let groups = result.instance_relationships().as_ref().unwrap().get(&name("servesAirports")).unwrap();
        assert_eq!(groups, &vec![vec![urn("urn:ngsi-ld:Airport:1")], vec![urn("urn:ngsi-ld:Airport:3")]]);
        let per_item = result.metadata().as_ref().unwrap().get(&name("servesAirports")).unwrap().as_per_item().unwrap();
        assert_eq!(per_item.len(), 2);
        assert_eq!(
            per_item[0].get(&name("datasetId")).map(SubAttribute::value),
            Some(&json!("urn:ngsi-ld:dataset:role:departure"))
        );
        assert_eq!(
            per_item[1].get(&name("datasetId")).map(SubAttribute::value),
            Some(&json!("urn:ngsi-ld:dataset:role:arrival"))
        );
    }

    #[test]
    fn a_nested_mapping_builds_a_structured_object() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Building",
                identity: { entityName: "Building-{{ id }}" },
                attributes: {
                    address: {
                        mappings: {
                            city: { source: "{{ city }}" },
                            street: { source: "{{ street }}" },
                        },
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(
            entity("urn:ngsi-ld:Building:1", json!({"id": "1", "city": "Ljubljana", "street": "Slovenska"})),
            mapping,
        );

        let (result, _) = extractor
            .extract(mapped, &DroppedGeometries::new(), &UnreadableTimestamps::new())
            .unwrap()
            .into_parts();
        let values = result.values().as_ref().expect("values set");

        match values.get(&name("address")) {
            Some(Value::Object(object)) => {
                assert_eq!(object.get("city"), Some(&Value::String("Ljubljana".into())));
                assert_eq!(object.get("street"), Some(&Value::String("Slovenska".into())));
            }
            other => panic!("expected an object value, got {other:?}"),
        }
    }

    #[test]
    fn a_relationship_contributes_no_value_entry() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Device",
                identity: { entityName: "Device-{{ id }}" },
                attributes: { controlledAsset: { type: "Relationship" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mut relationships = Relationships::default();
        relationships.insert(name("controlledAsset"), vec![urn("urn:ngsi-ld:Asset:9")]);
        let entity = Entity::new(urn("urn:ngsi-ld:Device:1"), json!({"id": "1"}), None, relationships, None);
        let mapped = AssembledEntity::from_single(entity, mapping);

        let (result, _) = extractor
            .extract(mapped, &DroppedGeometries::new(), &UnreadableTimestamps::new())
            .unwrap()
            .into_parts();
        let values = result.values().as_ref().expect("values set");

        assert!(!values.contains_key(&name("controlledAsset")));
    }

    const MOVIE: &str = r#"{
        version: "v4",
        dataModel: "Movie",
        identity: { entityName: "M-{{ id }}" },
        attributes: {
            hasLeadActor: {
                type: "Relationship",
                target: { entity: "Person" },
                source: "{{ actor }}",
                properties: {
                    playsCharacter: { type: "Relationship", target: { entity: "Character" }, source: "{{ character }}" },
                    billingOrder: { source: "{{ order }}", transformation: "int" },
                },
            },
        },
    }"#;

    #[test]
    fn a_nested_relationship_sub_attribute_reads_its_pre_minted_object() {
        let (resolver, mapping) = prepare(MOVIE);
        let mut relationships = Relationships::default();
        relationships.insert(name("hasLeadActor"), vec![urn("urn:ngsi-ld:Person:31")]);
        let mut nested = NestedRelationships::default();
        nested.insert(nested_path(&["hasLeadActor", "playsCharacter"]), vec![urn("urn:ngsi-ld:Character:JackSparrow")]);
        let mut entity = entity("urn:ngsi-ld:Movie:1", json!({"id": "1", "actor": "31", "character": "JackSparrow", "order": 0}));
        entity.relationships_mut().extend(relationships);
        entity.set_nested_relationships(Some(nested));

        let (result, _) = EntityExtractor::new(resolver)
            .extract(
                AssembledEntity::from_single(entity, mapping),
                &DroppedGeometries::new(),
                &UnreadableTimestamps::new(),
            )
            .unwrap()
            .into_parts();

        let shared = result.metadata().as_ref().unwrap().get(&name("hasLeadActor")).unwrap().as_shared().unwrap();
        let plays = shared.get(&name("playsCharacter")).expect("nested relationship sub-attribute");
        assert_eq!(*plays.kind(), NgsiLdAttributeKind::Relationship);
        assert_eq!(plays.value(), &json!("urn:ngsi-ld:Character:JackSparrow"));
        assert_eq!(plays.object_type().as_ref().map(NameBuf::as_str), Some("Character"));
        // The sibling Property sub-attribute is resolved as before.
        assert!(shared.contains_key(&name("billingOrder")));
    }

    #[test]
    fn a_relationship_of_relationship_carries_its_own_nested_relationship_sub_attribute() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Movie",
                identity: { entityName: "M-{{ id }}" },
                attributes: {
                    hasLeadActor: {
                        type: "Relationship",
                        target: { entity: "Person" },
                        source: "{{ actor }}",
                        properties: {
                            playsCharacter: {
                                type: "Relationship",
                                target: { entity: "Character" },
                                source: "{{ character }}",
                                properties: {
                                    locatedIn: { type: "Relationship", target: { entity: "Place" }, source: "{{ place }}" },
                                },
                            },
                        },
                    },
                },
            }"#,
        );
        let mut relationships = Relationships::default();
        relationships.insert(name("hasLeadActor"), vec![urn("urn:ngsi-ld:Person:31")]);
        let mut nested = NestedRelationships::default();
        nested.insert(nested_path(&["hasLeadActor", "playsCharacter"]), vec![urn("urn:ngsi-ld:Character:X")]);
        nested.insert(nested_path(&["hasLeadActor", "playsCharacter", "locatedIn"]), vec![urn("urn:ngsi-ld:Place:Y")]);
        let mut entity = entity("urn:ngsi-ld:Movie:1", json!({"id": "1", "actor": "31", "character": "X", "place": "Y"}));
        entity.relationships_mut().extend(relationships);
        entity.set_nested_relationships(Some(nested));

        let (result, _) = EntityExtractor::new(resolver)
            .extract(
                AssembledEntity::from_single(entity, mapping),
                &DroppedGeometries::new(),
                &UnreadableTimestamps::new(),
            )
            .unwrap()
            .into_parts();

        let shared = result.metadata().as_ref().unwrap().get(&name("hasLeadActor")).unwrap().as_shared().unwrap();
        let plays = shared.get(&name("playsCharacter")).expect("nested relationship");
        let located = plays.metadata().get(&name("locatedIn")).expect("doubly nested relationship");
        assert_eq!(*located.kind(), NgsiLdAttributeKind::Relationship);
        assert_eq!(located.value(), &json!("urn:ngsi-ld:Place:Y"));
        assert_eq!(located.object_type().as_ref().map(NameBuf::as_str), Some("Place"));
    }

    #[test]
    fn a_nested_list_relationship_sub_attribute_reads_all_its_objects() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Movie",
                identity: { entityName: "M-{{ id }}" },
                attributes: {
                    directedBy: {
                        type: "Relationship",
                        target: { entity: "Person" },
                        source: "{{ director }}",
                        properties: {
                            knownFor: { type: "ListRelationship", target: { entity: "Movie" }, source: "{{ films }}" },
                        },
                    },
                },
            }"#,
        );
        let mut relationships = Relationships::default();
        relationships.insert(name("directedBy"), vec![urn("urn:ngsi-ld:Person:5")]);
        let mut nested = NestedRelationships::default();
        nested.insert(
            nested_path(&["directedBy", "knownFor"]),
            vec![urn("urn:ngsi-ld:Movie:10"), urn("urn:ngsi-ld:Movie:20")],
        );
        let mut entity = entity("urn:ngsi-ld:Movie:1", json!({"id": "1", "director": "5", "films": "10 20"}));
        entity.relationships_mut().extend(relationships);
        entity.set_nested_relationships(Some(nested));

        let (result, _) = EntityExtractor::new(resolver)
            .extract(
                AssembledEntity::from_single(entity, mapping),
                &DroppedGeometries::new(),
                &UnreadableTimestamps::new(),
            )
            .unwrap()
            .into_parts();

        let shared = result.metadata().as_ref().unwrap().get(&name("directedBy")).unwrap().as_shared().unwrap();
        let known = shared.get(&name("knownFor")).expect("nested list relationship");
        assert_eq!(*known.kind(), NgsiLdAttributeKind::ListRelationship);
        assert_eq!(known.value(), &json!(["urn:ngsi-ld:Movie:10", "urn:ngsi-ld:Movie:20"]));
    }

    #[test]
    fn a_nested_relationship_absent_from_the_entity_is_dropped() {
        let (resolver, mapping) = prepare(MOVIE);
        let mut relationships = Relationships::default();
        relationships.insert(name("hasLeadActor"), vec![urn("urn:ngsi-ld:Person:31")]);
        // The nested map exists (a decoy path), but not the playsCharacter path, so it is dropped while
        // the sibling Property survives.
        let mut nested = NestedRelationships::default();
        nested.insert(nested_path(&["hasLeadActor", "somethingElse"]), vec![urn("urn:ngsi-ld:Other:9")]);
        let mut entity = entity("urn:ngsi-ld:Movie:1", json!({"id": "1", "actor": "31", "order": 0}));
        entity.relationships_mut().extend(relationships);
        entity.set_nested_relationships(Some(nested));

        let (result, _) = EntityExtractor::new(resolver)
            .extract(
                AssembledEntity::from_single(entity, mapping),
                &DroppedGeometries::new(),
                &UnreadableTimestamps::new(),
            )
            .unwrap()
            .into_parts();

        let shared = result.metadata().as_ref().unwrap().get(&name("hasLeadActor")).unwrap().as_shared().unwrap();
        assert!(!shared.contains_key(&name("playsCharacter")));
        assert!(shared.contains_key(&name("billingOrder")));
    }

    #[test]
    fn a_top_level_list_relationship_shared_property_becomes_shared_metadata() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Movie",
                identity: { entityName: "M-{{ id }}" },
                attributes: {
                    hasCast: {
                        type: "ListRelationship",
                        target: { entity: "Person" },
                        source: "{{ cast }}",
                        properties: { castSize: { source: "{{ size }}", transformation: "int" } },
                    },
                },
            }"#,
        );
        let mut relationships = Relationships::default();
        relationships.insert(name("hasCast"), vec![urn("urn:ngsi-ld:Person:1"), urn("urn:ngsi-ld:Person:2")]);
        let mut entity = entity("urn:ngsi-ld:Movie:1", json!({"id": "1", "cast": "1 2", "size": 2}));
        entity.relationships_mut().extend(relationships);

        let (result, _) = EntityExtractor::new(resolver)
            .extract(
                AssembledEntity::from_single(entity, mapping),
                &DroppedGeometries::new(),
                &UnreadableTimestamps::new(),
            )
            .unwrap()
            .into_parts();

        // A plain list relationship's sub-attribute qualifies the whole list, so it is shared metadata,
        // not per-item.
        let shared = result
            .metadata()
            .as_ref()
            .unwrap()
            .get(&name("hasCast"))
            .unwrap()
            .as_shared()
            .expect("shared metadata");
        assert!(shared.contains_key(&name("castSize")));
    }

    #[test]
    fn a_sequential_batch_extracts_every_entity() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "Sensor-{{ id }}" },
                attributes: { label: { source: "{{ id }}", transformation: "string" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver).with_parallelism(Parallelism::Sequential);
        let batch = vec![
            AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:1", json!({"id": "1"})), Arc::clone(&mapping)),
            AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:2", json!({"id": "2"})), mapping),
        ];

        let results = extractor.extract_batch(batch, &DroppedGeometries::new(), &UnreadableTimestamps::new());

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(Result::is_ok));
    }

    #[test]
    fn a_parallel_batch_extracts_every_entity() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "Sensor-{{ id }}" },
                attributes: { label: { source: "{{ id }}", transformation: "string" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver).with_parallelism(Parallelism::Parallel);
        let batch = vec![
            AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:1", json!({"id": "1"})), Arc::clone(&mapping)),
            AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:2", json!({"id": "2"})), mapping),
        ];

        let results = extractor.extract_batch(batch, &DroppedGeometries::new(), &UnreadableTimestamps::new());

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(Result::is_ok));
    }
}
