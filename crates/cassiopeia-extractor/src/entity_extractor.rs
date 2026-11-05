use crate::{
    attribute::{resolution_context::ResolutionContext, resolution_error::ResolutionError, resolver::resolve},
    dropped_attributes::DroppedAttributes,
    error::{ExtractionError, Result},
    extractor::Extractor,
};
use cassiopeia_common::parallelism::Parallelism;
use cassiopeia_ir::{
    assembled_entity::{AssembledEntity, AssembledEntityParts},
    entity::{AttributeValues, Entity},
    entity_relationships::EntityRelationshipsParts,
    mapped::{Mapped, Mappings},
    metadata::EntityMetadata,
};
use cassiopeia_mapping::template::resolver::TemplateResolver;
use rayon::prelude::*;
use serde_json::Value as JsonValue;
use std::sync::Arc;

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
}

impl Extractor for EntityExtractor {
    fn extract(&self, assembled: AssembledEntity, dropped: &DroppedAttributes) -> Result<Mapped<Entity>> {
        let AssembledEntityParts {
            id,
            scope,
            expires_at,
            mut relationships,
            fragments,
        } = assembled.into_parts();
        let mut values = AttributeValues::default();
        let mut metadata = EntityMetadata::default();
        let mappings: Mappings = fragments.iter().map(|(_, mapping)| Arc::clone(mapping)).collect();

        // Each fragment resolves through its own mapping against its own record; disjoint attribute
        // names across mappings make the union a concatenation, last-writer-wins on the rare collision.
        for (data, mapping) in &fragments {
            let mut unresolved = Vec::new();
            {
                let mut context = ResolutionContext::new(data, &self.resolver, &relationships, dropped, Some(&mut metadata));
                for (name, config) in mapping.attributes() {
                    match resolve(&mut context, name, config) {
                        Ok(None) => {}
                        Ok(Some(resolved)) => {
                            values.insert(name.clone(), resolved);
                        }
                        // A template failure costs the attribute it belongs to, not the entity.
                        // Resolution records an attribute's metadata only once the attribute has
                        // resolved whole, so nothing of the failed attribute is left behind.
                        Err(ResolutionError::Template { source }) => {
                            dropped.templates.record(name, &id, source);
                            unresolved.push(name);
                        }
                        Err(ResolutionError::RecursionLimitExceeded { depth }) => return Err(ExtractionError::RecursionLimitExceeded { depth }),
                    }
                }
            }
            // A relationship's objects were minted upstream and travel on the entity, so a
            // relationship whose properties failed to resolve is removed from them as well; otherwise
            // it would be emitted stripped of the properties its mapping declared.
            for name in unresolved {
                relationships.remove_attribute(name);
            }
        }

        // The data is spent once every fragment is resolved; the transformer reads only the resolved
        // maps and the carried mappings, so the entity's own record is no longer needed. Nested
        // objects were folded into the metadata as sub-attributes, so only the top-level and the
        // per-instance objects travel on.
        let EntityRelationshipsParts {
            top_level: relationships,
            instances: instance_relationships,
            ..
        } = relationships.into_parts();
        let mut entity = Entity::new(id, JsonValue::Null, scope, expires_at, relationships, Some(values));
        if !metadata.is_empty() {
            entity.set_metadata(Some(metadata));
        }
        entity.set_instance_relationships(instance_relationships.filter(|instances| !instances.is_empty()));

        Ok(Mapped::with_mappings(entity, mappings))
    }

    fn extract_batch(&self, entities: Vec<AssembledEntity>, dropped: &DroppedAttributes) -> Vec<Result<Mapped<Entity>>> {
        match self.parallelism {
            Parallelism::Parallel => entities.into_par_iter().map(|entity| self.extract(entity, dropped)).collect(),
            Parallelism::Sequential => entities.into_iter().map(|entity| self.extract(entity, dropped)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        dropped_attributes::DroppedAttributes,
        dropped_null::{DroppedNull, NullSite},
        entity_extractor::EntityExtractor,
        extractor::Extractor,
        required_value::RequiredValue,
    };
    use cassiopeia_batch_sink::{batch_sink::BatchSink, recurring::Recurring};
    use cassiopeia_common::parallelism::Parallelism;
    use cassiopeia_expander::compiler::ExpanderCompiler;
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use cassiopeia_ir::{
        assembled_entity::AssembledEntity,
        attribute_content::AttributeContent,
        entity::Entity,
        instance_index::InstanceIndex,
        metadata::MetadataStorage,
        relationship_path::RelationshipPath,
        relationships::{InstanceObjects, InstanceRelationships, NestedRelationships, Relationships},
        resolved_attribute::ResolvedAttribute,
        sub_attribute::SubAttribute,
    };
    use cassiopeia_mapping::{
        mapping::Mapping,
        template::{error::ResolutionFailure, resolver::TemplateResolver, runner::TemplateRunner},
        template_site::TemplateSite,
    };
    use cassiopeia_ngsi_ld::{
        entity::{
            attribute::{NgsiLdAttributeKind, language_property::LanguageMap},
            name::NameBuf,
        },
        value::{
            json_payload_error::{JsonPayloadError, JsonShape},
            types::{Number, TemporalValue, Value},
        },
    };
    use cassiopeia_unreadable_timestamps::{attribute_loss::AttributeLoss, timestamp_loss::TimestampLoss};
    use chrono::{TimeZone, Utc};
    use iri_rs::IriBuf;
    use langtag::LangTagBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    /// Parses a mapping document and compiles its templates, returning both the shared resolver and
    /// the loaded mapping: the same preparation the pipeline performs before extraction.
    fn prepare(document: &str) -> (TemplateResolver, Arc<Mapping>) {
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, Path::new("test.json5"), &mut runner).unwrap();
        let resolver = runner.resolver();

        (resolver, Arc::new(mapping))
    }

    /// A single-instance attribute that resolved to `value`.
    fn single(value: Value) -> ResolvedAttribute {
        ResolvedAttribute::Single(AttributeContent::Value(value))
    }

    /// The NGSI-LD Value a single-instance attribute resolved to, if it resolved at all.
    fn single_value(resolved: Option<ResolvedAttribute>) -> Option<Value> {
        match resolved? {
            ResolvedAttribute::Single(AttributeContent::Value(value)) => Some(value),
            other @ (ResolvedAttribute::Single(AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_))
            | ResolvedAttribute::Instances(_)) => {
                panic!("expected a single value, got {other:?}")
            }
        }
    }

    fn urn(value: &str) -> Urn {
        value.parse::<Urn>().unwrap()
    }

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn entity(id: &str, data: JsonValue) -> Entity {
        Entity::new(urn(id), data, None, None, Relationships::default(), None)
    }

    fn nested_path(segments: &[&str]) -> RelationshipPath {
        RelationshipPath::from_segments(segments.iter().map(|segment| name(segment)).collect())
    }

    /// The objects each listed instance of one multi-attribute relationship minted, by declaration
    /// index.
    fn instance_objects(instances: &[(usize, &[&str])]) -> InstanceObjects {
        instances
            .iter()
            .map(|(index, objects)| (InstanceIndex::from(*index), objects.iter().map(|object| urn(object)).collect()))
            .collect()
    }

    /// An entity over `data` whose attribute `attribute` carries the given per-instance objects.
    fn entity_with_instances(id: &str, data: JsonValue, attribute: &str, objects: InstanceObjects) -> Entity {
        let mut instances = InstanceRelationships::default();
        instances.insert(name(attribute), objects);
        let mut entity = entity(id, data);
        entity.set_instance_relationships(Some(instances));
        entity
    }

    /// Every per-instance `datasetId` recorded for `attribute`, in declaration order.
    fn per_item_dataset_ids(result: &Entity, attribute: &str) -> Vec<Option<JsonValue>> {
        result
            .metadata()
            .as_ref()
            .unwrap()
            .get(&name(attribute))
            .unwrap()
            .as_per_item()
            .unwrap()
            .iter()
            .map(|item| {
                item.get(&name("datasetId"))
                    .and_then(|sub| sub.value().as_value())
                    .map(|value| serde_json::to_value(value).unwrap())
            })
            .collect()
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

        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();
        let values = result.values().as_ref().expect("values set");

        assert_eq!(values.get(&name("temperature")), Some(&single(Value::try_from(25.5).unwrap())));
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
        let dropped = DroppedAttributes::new();

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Zone:001", record), mapping);
        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();
        let values = result.values().as_ref().expect("values set");

        // The unconvertible attribute is gone, but every other attribute of the entity survives.
        assert!(!values.contains_key(&name("location")));
        assert!(values.contains_key(&name("reference")));

        let entries = BatchSink::from(dropped.geometries).into_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.attribute, name("location"));
        assert_eq!(entries[0].1.occurrences().get(), 1);
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
        let dropped = DroppedAttributes::new();

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
            let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();
            let values = result.values().as_ref().expect("values set").clone();

            let expected = TemporalValue::DateTime(Utc.with_ymd_and_hms(2026, 3, 1, 11, 4, 35).unwrap());
            assert_eq!(
                values.get(&name("reading")),
                Some(&single(Value::Temporal(expected))),
                "mismatch for {spelling:?}"
            );
        }

        assert!(dropped.timestamps.is_empty());
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
        let dropped = DroppedAttributes::new();
        let record = json!({"id": "001", "ts": "the third of March"});

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:001", record), mapping);
        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();
        let values = result.values().as_ref().expect("values set");

        // The unreadable attribute is gone, but every other attribute of the entity survives.
        assert!(!values.contains_key(&name("reading")));
        assert!(values.contains_key(&name("reference")));

        let entries = BatchSink::from(dropped.timestamps).into_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].0,
            AttributeLoss {
                loss: TimestampLoss::AttributeValue,
                attribute: name("reading")
            }
        );
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
        let dropped = DroppedAttributes::new();

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:001", json!({"id": "001", "ts": ""})), mapping);
        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();

        assert!(!result.values().as_ref().expect("values set").contains_key(&name("reading")));
        assert!(dropped.timestamps.is_empty());
    }

    #[test]
    fn a_composite_property_over_a_missing_null_or_empty_field_is_absent_rather_than_holding_the_text_null() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Person",
                identity: { entityName: "Person-{{ id }}" },
                attributes: { name: { source: "{{ first }} {{ last }}" }, reference: { source: "{{ id }}" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);

        for record in [
            json!({"id": "1", "first": "John"}),
            json!({"id": "1", "first": "John", "last": null}),
            json!({"id": "1", "first": "John", "last": ""}),
        ] {
            let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Person:1", record), Arc::clone(&mapping));
            let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();
            let values = result.values().as_ref().expect("values set");

            assert!(!values.contains_key(&name("name")), "{values:?}");
            assert!(values.contains_key(&name("reference")));
        }
    }

    #[test]
    fn a_composite_property_with_every_field_present_holds_the_joined_text() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Person",
                identity: { entityName: "Person-{{ id }}" },
                attributes: { name: { source: "{{ first }} {{ last }}" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Person:1", json!({"id": "1", "first": "John", "last": "Doe"})), mapping);
        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();

        assert_eq!(
            result.values().as_ref().expect("values set").get(&name("name")),
            Some(&single(Value::String("John Doe".into())))
        );
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
        let dropped = DroppedAttributes::new();

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Zone:001", record), mapping);
        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();
        let values = result.values().as_ref().expect("values set");

        assert!(values.contains_key(&name("location")));
        assert!(dropped.geometries.is_empty());
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

        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();
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

        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();
        let metadata = result.metadata().as_ref().expect("metadata set");
        let shared = metadata
            .get(&name("temperature"))
            .expect("temperature metadata")
            .as_shared()
            .expect("shared metadata");

        assert_eq!(
            shared.get(&name("unitCode")).map(SubAttribute::value),
            Some(&AttributeContent::Value(Value::try_from(json!("CEL")).unwrap()))
        );
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

        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();
        let metadata = result.metadata().as_ref().expect("metadata set");
        let shared = metadata.get(&name("sugars")).expect("sugars metadata").as_shared().expect("shared metadata");
        let level = shared.get(&name("level")).expect("level sub-attribute");

        assert_eq!(*level.kind(), NgsiLdAttributeKind::VocabProperty);
        assert_eq!(
            level.value(),
            &AttributeContent::Vocab(IriBuf::new("https://example.org/level/high".to_owned()).unwrap())
        );
    }

    #[test]
    fn unusable_vocab_and_language_values_drop_only_their_own_declarations_and_are_recorded_by_path() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Thing",
                identity: { entityName: "{{ id }}" },
                attributes: {
                    topVocab: { type: "VocabProperty", source: "{{ v }}" },
                    topLang: { type: "LanguageProperty", source: "{{ l }}" },
                    temperature: {
                        source: "{{ t }}",
                        properties: {
                            subVocab: { type: "VocabProperty", source: "{{ v }}" },
                            subLang: { type: "LanguageProperty", source: "{{ l }}" },
                        },
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(
            entity("urn:ngsi-ld:Thing:A", json!({"id": "A", "t": 20, "v": "not an iri", "l": "plain text"})),
            mapping,
        );
        let dropped = DroppedAttributes::new();

        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();

        let values = result.values().as_ref().expect("values set");
        assert_eq!(values.keys().map(NameBuf::as_str).collect::<Vec<_>>(), ["temperature"]);
        assert!(result.metadata().as_ref().and_then(|metadata| metadata.get(&name("temperature"))).is_none());
        let recorded = BatchSink::from(dropped.unusable_values)
            .into_entries()
            .map(|(unusable, record)| (unusable.attribute.to_string(), unusable.required, record.example.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(
            recorded,
            [
                ("topVocab".to_owned(), RequiredValue::Iri, "not an iri".to_owned()),
                ("topLang".to_owned(), RequiredValue::LanguageMap, "plain text".to_owned()),
                ("temperature.subVocab".to_owned(), RequiredValue::Iri, "not an iri".to_owned()),
                ("temperature.subLang".to_owned(), RequiredValue::LanguageMap, "plain text".to_owned()),
            ]
        );
    }

    #[test]
    fn a_refused_vocab_instance_leaves_its_index_empty_and_keeps_the_others() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "S-{{ id }}" },
                attributes: {
                    category: {
                        type: "VocabProperty",
                        instances: [ { source: "{{ a }}" }, { source: "{{ b }}" } ],
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(
            entity("urn:ngsi-ld:Sensor:1", json!({"id": "1", "a": "not an iri", "b": "https://example.org/b"})),
            mapping,
        );
        let dropped = DroppedAttributes::new();

        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();

        let category = result.values().as_ref().and_then(|values| values.get("category")).expect("category resolved");
        assert_eq!(
            category,
            &ResolvedAttribute::Instances(vec![
                None,
                Some(AttributeContent::Vocab(IriBuf::new("https://example.org/b".to_owned()).unwrap()))
            ])
        );
        assert_eq!(BatchSink::from(dropped.unusable_values).into_entries().count(), 1);
    }

    #[test]
    fn an_unreadable_integer_instance_leaves_its_index_empty_keeps_the_others_and_is_recorded() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "S-{{ id }}" },
                attributes: {
                    reading: {
                        transformation: "integer",
                        instances: [
                            { source: "{{ a }}", datasetId: "urn:ngsi-ld:Dataset:a" },
                            { source: "{{ b }}", datasetId: "urn:ngsi-ld:Dataset:b" },
                        ],
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Sensor:1", json!({"id": "1", "a": "x", "b": "7"})), mapping);
        let dropped = DroppedAttributes::new();

        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();

        let reading = result.values().as_ref().and_then(|values| values.get("reading")).expect("reading resolved");
        assert_eq!(
            reading,
            &ResolvedAttribute::Instances(vec![None, Some(AttributeContent::Value(Value::Number(Number::Integer(7))))])
        );
        let recorded = BatchSink::from(dropped.unusable_values)
            .into_entries()
            .map(|(unusable, record)| (unusable.attribute.to_string(), unusable.required, record.example.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(recorded, [("reading".to_owned(), RequiredValue::Integer, "x".to_owned())]);
    }

    #[test]
    fn a_language_property_assembled_from_nested_mappings_becomes_its_language_map() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Museum",
                identity: { entityName: "M-{{ id }}" },
                attributes: {
                    name: {
                        type: "LanguageProperty",
                        mappings: { fr: { source: "{{ nom }}" }, nl: { source: "{{ naam }}" } },
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Museum:1", json!({"id": "1", "nom": "Musée", "naam": "Museum"})), mapping);
        let dropped = DroppedAttributes::new();

        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();

        let ResolvedAttribute::Single(AttributeContent::Language(map)) = result.values().as_ref().and_then(|values| values.get("name")).expect("name resolved")
        else {
            panic!("expected a language map");
        };
        assert_eq!(
            map.iter().map(|(tag, text)| (tag.as_str(), text.as_str())).collect::<Vec<_>>(),
            [("fr", "Musée"), ("nl", "Museum")]
        );
        assert!(dropped.unusable_values.is_empty());
    }

    /// What a `LanguageProperty` named `name`, read from `names` with no transformation, resolves to
    /// over one record carrying `names`, with the losses recorded.
    fn resolve_language_source(names: &JsonValue) -> (Option<ResolvedAttribute>, DroppedAttributes) {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Museum",
                identity: { entityName: "M-{{ id }}" },
                attributes: {
                    name: { type: "LanguageProperty", source: "{{ names }}" },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Museum:1", json!({"id": "1", "names": names})), mapping);
        let dropped = DroppedAttributes::new();

        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();

        (result.values().as_ref().and_then(|values| values.get("name")).cloned(), dropped)
    }

    /// A `languageMap` holding `entries`, each a language tag and its text.
    fn language_map(entries: &[(&str, &str)]) -> LanguageMap {
        entries
            .iter()
            .map(|(tag, text)| (LangTagBuf::new((*tag).to_owned()).unwrap(), (*text).to_owned()))
            .collect()
    }

    #[test]
    fn a_language_property_reads_an_object_of_tagged_text_by_default() {
        let (resolved, dropped) = resolve_language_source(&json!({"en": "Hello", "de": "Hallo"}));

        assert_eq!(
            resolved,
            Some(ResolvedAttribute::Single(AttributeContent::Language(language_map(&[
                ("en", "Hello"),
                ("de", "Hallo")
            ]))))
        );
        assert!(dropped.unusable_values.is_empty());
    }

    #[test]
    fn a_language_property_reads_the_json_text_of_its_map_by_default() {
        let (resolved, dropped) = resolve_language_source(&json!(r#"{"en":"Hello"}"#));

        assert_eq!(
            resolved,
            Some(ResolvedAttribute::Single(AttributeContent::Language(language_map(&[("en", "Hello")]))))
        );
        assert!(dropped.unusable_values.is_empty());
    }

    #[test]
    fn plain_text_on_a_language_property_is_refused_as_no_language_map() {
        let (resolved, dropped) = resolve_language_source(&json!("Hello"));

        assert_eq!(resolved, None);
        let recorded = BatchSink::from(dropped.unusable_values)
            .into_entries()
            .map(|(unusable, record)| (unusable.attribute.to_string(), unusable.required, record.example.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(recorded, [("name".to_owned(), RequiredValue::LanguageMap, "Hello".to_owned())]);
    }

    #[test]
    fn a_blank_language_property_source_is_absent_rather_than_refused() {
        let (resolved, dropped) = resolve_language_source(&json!("  "));

        assert_eq!(resolved, None);
        assert!(dropped.unusable_values.is_empty());
    }

    #[test]
    fn a_language_property_read_whole_under_an_object_transformation_becomes_its_language_map() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Museum",
                identity: { entityName: "M-{{ id }}" },
                attributes: {
                    name: { type: "LanguageProperty", transformation: "object", source: "{{ names }}" },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(
            entity("urn:ngsi-ld:Museum:1", json!({"id": "1", "names": {"fr": "Musée", "nl": "Museum"}})),
            mapping,
        );
        let dropped = DroppedAttributes::new();

        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();

        let ResolvedAttribute::Single(AttributeContent::Language(map)) = result.values().as_ref().and_then(|values| values.get("name")).expect("name resolved")
        else {
            panic!("expected a language map");
        };
        assert_eq!(map.len(), 2);
        assert!(dropped.unusable_values.is_empty());
    }

    #[test]
    fn a_member_of_a_structured_value_is_recorded_by_its_bracketed_path() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Building",
                identity: { entityName: "B-{{ id }}" },
                attributes: {
                    address: {
                        mappings: { kind: { type: "VocabProperty", source: "{{ kind }}" }, street: { source: "{{ street }}" } },
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(
            entity("urn:ngsi-ld:Building:1", json!({"id": "1", "kind": "not an iri", "street": "Main"})),
            mapping,
        );
        let dropped = DroppedAttributes::new();

        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();

        assert_eq!(
            single_value(result.values().as_ref().and_then(|values| values.get("address")).cloned()),
            Some(Value::try_from(json!({"street": "Main"})).unwrap())
        );
        let paths = BatchSink::from(dropped.unusable_values)
            .into_entries()
            .map(|(unusable, _)| unusable.attribute.to_string())
            .collect::<Vec<_>>();
        assert_eq!(paths, ["address[kind]"]);
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

        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();
        let metadata = result.metadata().as_ref().expect("metadata set");
        let shared = metadata.get(&name("sugars")).expect("sugars metadata").as_shared().expect("shared metadata");
        let level = shared.get(&name("level")).expect("level sub-attribute");
        let inner = level.metadata().get(&name("source")).expect("nested sub-attribute");

        assert_eq!(*level.kind(), NgsiLdAttributeKind::VocabProperty);
        assert_eq!(inner.value(), &AttributeContent::Value(Value::try_from(json!("measured")).unwrap()));
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

        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();
        let values = result.values().as_ref().expect("values set");
        let metadata = result.metadata().as_ref().expect("metadata set");

        assert_eq!(
            values.get(&name("temperatureMax")),
            Some(&ResolvedAttribute::Instances(vec![
                Some(AttributeContent::Value(Value::try_from(10.0).unwrap())),
                Some(AttributeContent::Value(Value::try_from(12.0).unwrap())),
            ]))
        );

        let per_item = metadata
            .get(&name("temperatureMax"))
            .expect("temperatureMax metadata")
            .as_per_item()
            .expect("per-item metadata");
        assert_eq!(per_item.len(), 2);
        // Each instance carries the shared unitCode plus its own datasetId.
        assert_eq!(
            per_item[0].get(&name("unitCode")).map(SubAttribute::value),
            Some(&AttributeContent::Value(Value::try_from(json!("CEL")).unwrap()))
        );
        assert_eq!(
            per_item[0].get(&name("datasetId")).map(SubAttribute::value),
            Some(&AttributeContent::Value(Value::try_from(json!("urn:ngsi-ld:dataset:model:a")).unwrap()))
        );
        assert_eq!(
            per_item[1].get(&name("datasetId")).map(SubAttribute::value),
            Some(&AttributeContent::Value(Value::try_from(json!("urn:ngsi-ld:dataset:model:b")).unwrap()))
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
        // The expander already minted one object per instance, each under its own instance; the
        // extractor records the per-instance metadata at the same indices.
        let entity = entity_with_instances(
            "urn:ngsi-ld:Flight:1",
            json!({"id": "1", "dep": "535", "arr": "340"}),
            "servesAirport",
            instance_objects(&[(0, &["urn:ngsi-ld:Airport:535"]), (1, &["urn:ngsi-ld:Airport:340"])]),
        );

        let (result, _) = EntityExtractor::new(resolver)
            .extract(AssembledEntity::from_single(entity, mapping), &DroppedAttributes::new())
            .unwrap()
            .into_parts();

        assert!(!result.values().as_ref().unwrap().contains_key(&name("servesAirport")));
        assert_eq!(
            per_item_dataset_ids(&result, "servesAirport"),
            [
                Some(json!("urn:ngsi-ld:dataset:role:departure")),
                Some(json!("urn:ngsi-ld:dataset:role:arrival"))
            ]
        );
    }

    #[test]
    fn a_relationship_instance_with_no_object_keeps_every_instance_on_its_own_metadata_index() {
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
        // The middle instance's key `b` is absent, so the expander minted nothing under index 1. Its
        // metadata entry stays, so the arrival instance's objects still meet the arrival metadata.
        let entity = entity_with_instances(
            "urn:ngsi-ld:Flight:1",
            json!({"id": "1", "a": "1", "c": "3"}),
            "servesAirport",
            instance_objects(&[(0, &["urn:ngsi-ld:Airport:1"]), (2, &["urn:ngsi-ld:Airport:3"])]),
        );

        let (result, _) = EntityExtractor::new(resolver)
            .extract(AssembledEntity::from_single(entity, mapping), &DroppedAttributes::new())
            .unwrap()
            .into_parts();

        assert_eq!(
            per_item_dataset_ids(&result, "servesAirport"),
            [
                Some(json!("urn:ngsi-ld:dataset:role:departure")),
                Some(json!("urn:ngsi-ld:dataset:role:via")),
                Some(json!("urn:ngsi-ld:dataset:role:arrival")),
            ]
        );
        assert_eq!(
            result.instance_relationships().as_ref().unwrap().get(&name("servesAirport")),
            Some(&instance_objects(&[(0, &["urn:ngsi-ld:Airport:1"]), (2, &["urn:ngsi-ld:Airport:3"])]))
        );
    }

    #[test]
    fn list_relationship_instance_objects_travel_on_beside_their_per_instance_metadata() {
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
        // The record's text is deliberately unrelated to the objects: the extractor never re-reads an
        // instance's source, it carries the objects the expander minted under each instance.
        let objects = instance_objects(&[(0, &["urn:ngsi-ld:Airport:1", "urn:ngsi-ld:Airport:2"]), (1, &["urn:ngsi-ld:Airport:3"])]);
        let entity = entity_with_instances(
            "urn:ngsi-ld:Route:1",
            json!({"id": "1", "a": "• 9 8 7", "b": ""}),
            "servesAirports",
            objects.clone(),
        );

        let (result, _) = EntityExtractor::new(resolver)
            .extract(AssembledEntity::from_single(entity, mapping), &DroppedAttributes::new())
            .unwrap()
            .into_parts();

        assert_eq!(result.instance_relationships().as_ref().unwrap().get(&name("servesAirports")), Some(&objects));
        assert!(!result.relationships().contains_key(&name("servesAirports")));
        assert_eq!(
            per_item_dataset_ids(&result, "servesAirports"),
            [
                Some(json!("urn:ngsi-ld:dataset:role:departure")),
                Some(json!("urn:ngsi-ld:dataset:role:arrival"))
            ]
        );
    }

    #[test]
    fn a_list_relationship_instance_with_no_objects_keeps_every_instance_on_its_own_metadata_index() {
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
        // The middle instance minted no object (its tokens named no target), so index 1 has none; the
        // arrival objects stay under index 2, beside the arrival metadata.
        let objects = instance_objects(&[(0, &["urn:ngsi-ld:Airport:1"]), (2, &["urn:ngsi-ld:Airport:3"])]);
        let entity = entity_with_instances(
            "urn:ngsi-ld:Route:1",
            json!({"id": "1", "a": "1", "b": "• ?", "c": "3"}),
            "servesAirports",
            objects.clone(),
        );

        let (result, _) = EntityExtractor::new(resolver)
            .extract(AssembledEntity::from_single(entity, mapping), &DroppedAttributes::new())
            .unwrap()
            .into_parts();

        assert_eq!(result.instance_relationships().as_ref().unwrap().get(&name("servesAirports")), Some(&objects));
        assert_eq!(
            per_item_dataset_ids(&result, "servesAirports"),
            [
                Some(json!("urn:ngsi-ld:dataset:role:departure")),
                Some(json!("urn:ngsi-ld:dataset:role:via")),
                Some(json!("urn:ngsi-ld:dataset:role:arrival")),
            ]
        );
    }

    #[test]
    fn a_relationship_instance_attribute_with_no_objects_records_no_metadata() {
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
                        ],
                    },
                },
            }"#,
        );

        let (result, _) = EntityExtractor::new(resolver)
            .extract(
                AssembledEntity::from_single(entity("urn:ngsi-ld:Route:1", json!({"id": "1", "a": "•"})), mapping),
                &DroppedAttributes::new(),
            )
            .unwrap()
            .into_parts();

        assert_eq!(result.metadata(), &None);
        assert_eq!(result.instance_relationships(), &None);
    }

    #[test]
    fn a_failed_relationship_instance_property_drops_the_instance_objects_with_it() {
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
                            { source: "{{ a }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:departure" }, since: { source: "{{ since | upper }}" } } },
                        ],
                    },
                },
            }"#,
        );
        let dropped = DroppedAttributes::new();
        let entity = entity_with_instances(
            "urn:ngsi-ld:Route:1",
            json!({"id": "1", "a": "1", "since": 2020}),
            "servesAirports",
            instance_objects(&[(0, &["urn:ngsi-ld:Airport:1"])]),
        );

        let (result, _) = EntityExtractor::new(resolver)
            .extract(AssembledEntity::from_single(entity, mapping), &dropped)
            .unwrap()
            .into_parts();

        assert_eq!(result.instance_relationships(), &None);
        assert_eq!(
            BatchSink::from(dropped.templates).into_entries().collect::<Vec<_>>()[0].0,
            name("servesAirports")
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

        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();
        let values = result.values().as_ref().expect("values set");

        match single_value(values.get(&name("address")).cloned()) {
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
        let entity = Entity::new(urn("urn:ngsi-ld:Device:1"), json!({"id": "1"}), None, None, relationships, None);
        let mapped = AssembledEntity::from_single(entity, mapping);

        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();
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
        let mut entity = Entity::new(
            urn("urn:ngsi-ld:Movie:1"),
            json!({"id": "1", "actor": "31", "character": "JackSparrow", "order": 0}),
            None,
            None,
            relationships,
            None,
        );
        entity.set_nested_relationships(Some(nested));

        let (result, _) = EntityExtractor::new(resolver)
            .extract(AssembledEntity::from_single(entity, mapping), &DroppedAttributes::new())
            .unwrap()
            .into_parts();

        let shared = result.metadata().as_ref().unwrap().get(&name("hasLeadActor")).unwrap().as_shared().unwrap();
        let plays = shared.get(&name("playsCharacter")).expect("nested relationship sub-attribute");
        assert_eq!(*plays.kind(), NgsiLdAttributeKind::Relationship);
        assert_eq!(
            plays.value(),
            &AttributeContent::Value(Value::try_from(json!("urn:ngsi-ld:Character:JackSparrow")).unwrap())
        );
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
        let mut entity = Entity::new(
            urn("urn:ngsi-ld:Movie:1"),
            json!({"id": "1", "actor": "31", "character": "X", "place": "Y"}),
            None,
            None,
            relationships,
            None,
        );
        entity.set_nested_relationships(Some(nested));

        let (result, _) = EntityExtractor::new(resolver)
            .extract(AssembledEntity::from_single(entity, mapping), &DroppedAttributes::new())
            .unwrap()
            .into_parts();

        let shared = result.metadata().as_ref().unwrap().get(&name("hasLeadActor")).unwrap().as_shared().unwrap();
        let plays = shared.get(&name("playsCharacter")).expect("nested relationship");
        let located = plays.metadata().get(&name("locatedIn")).expect("doubly nested relationship");
        assert_eq!(*located.kind(), NgsiLdAttributeKind::Relationship);
        assert_eq!(
            located.value(),
            &AttributeContent::Value(Value::try_from(json!("urn:ngsi-ld:Place:Y")).unwrap())
        );
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
        let mut entity = Entity::new(
            urn("urn:ngsi-ld:Movie:1"),
            json!({"id": "1", "director": "5", "films": "10 20"}),
            None,
            None,
            relationships,
            None,
        );
        entity.set_nested_relationships(Some(nested));

        let (result, _) = EntityExtractor::new(resolver)
            .extract(AssembledEntity::from_single(entity, mapping), &DroppedAttributes::new())
            .unwrap()
            .into_parts();

        let shared = result.metadata().as_ref().unwrap().get(&name("directedBy")).unwrap().as_shared().unwrap();
        let known = shared.get(&name("knownFor")).expect("nested list relationship");
        assert_eq!(*known.kind(), NgsiLdAttributeKind::ListRelationship);
        assert_eq!(
            known.value(),
            &AttributeContent::Value(Value::try_from(json!(["urn:ngsi-ld:Movie:10", "urn:ngsi-ld:Movie:20"])).unwrap())
        );
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
        let mut entity = Entity::new(
            urn("urn:ngsi-ld:Movie:1"),
            json!({"id": "1", "actor": "31", "order": 0}),
            None,
            None,
            relationships,
            None,
        );
        entity.set_nested_relationships(Some(nested));

        let (result, _) = EntityExtractor::new(resolver)
            .extract(AssembledEntity::from_single(entity, mapping), &DroppedAttributes::new())
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
        let entity = Entity::new(
            urn("urn:ngsi-ld:Movie:1"),
            json!({"id": "1", "cast": "1 2", "size": 2}),
            None,
            None,
            relationships,
            None,
        );

        let (result, _) = EntityExtractor::new(resolver)
            .extract(AssembledEntity::from_single(entity, mapping), &DroppedAttributes::new())
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

        let results = extractor.extract_batch(batch, &DroppedAttributes::new());

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

        let results = extractor.extract_batch(batch, &DroppedAttributes::new());

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(Result::is_ok));
    }

    /// A headerless country mapping whose `dafifCode` is built by `source` under `transformation`.
    fn country_mapping(source: &str, transformation: &str) -> (TemplateResolver, Arc<Mapping>) {
        prepare(&format!(
            r#"{{
                version: "v4",
                dataModel: "Country",
                identity: {{ entityName: "{{{{ this[0] }}}}" }},
                attributes: {{ dafifCode: {{ source: {source:?}, type: "Property", transformation: "{transformation}" }} }},
            }}"#
        ))
    }

    /// Extracts one headerless country record and returns its `dafifCode` value, if any, with the
    /// sinks the extraction recorded into. Extraction itself must succeed: the entity is produced.
    fn extract_dafif_code(source: &str, transformation: &str, codes: &JsonValue) -> (Option<Value>, DroppedAttributes) {
        let (resolver, mapping) = country_mapping(source, transformation);
        let extractor = EntityExtractor::new(resolver);
        let dropped = DroppedAttributes::new();
        let record = json!({"0": "Bonaire", "1": "BQ", "2": codes});

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Country:Bonaire", record), mapping);
        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();
        let value = single_value(result.values().as_ref().expect("values set").get(&name("dafifCode")).cloned());

        (value, dropped)
    }

    const SPLIT: &str = r#"{{ this[2] | split(pat=" ") }}"#;
    const GUARDED_SPLIT: &str = r#"{% if this[2] %}{{ this[2] | split(pat=" ") }}{% endif %}"#;

    #[test]
    fn a_split_text_column_becomes_an_array_of_its_tokens() {
        let (value, _) = extract_dafif_code(SPLIT, "array", &json!("BS IN"));

        assert_eq!(value, Some(Value::try_from(json!(["BS", "IN"])).unwrap()));
    }

    #[test]
    fn a_split_single_token_becomes_a_one_element_array() {
        let (value, _) = extract_dafif_code(SPLIT, "array", &json!("UK"));

        assert_eq!(value, Some(Value::try_from(json!(["UK"])).unwrap()));
    }

    #[test]
    fn a_json_decoded_text_column_becomes_the_array_it_encodes() {
        let (value, _) = extract_dafif_code("{{ this[2] | json_decode }}", "array", &json!(r#"["BS","IN"]"#));

        assert_eq!(value, Some(Value::try_from(json!(["BS", "IN"])).unwrap()));
    }

    #[test]
    fn a_guarded_split_over_a_null_column_yields_no_attribute_and_keeps_the_entity() {
        let (value, dropped) = extract_dafif_code(GUARDED_SPLIT, "array", &JsonValue::Null);

        assert_eq!(value, None);
        assert!(dropped.templates.is_empty());
    }

    #[test]
    fn a_guarded_split_over_an_empty_column_yields_no_attribute_and_keeps_the_entity() {
        let (value, dropped) = extract_dafif_code(GUARDED_SPLIT, "array", &json!(""));

        assert_eq!(value, None);
        assert!(dropped.templates.is_empty());
    }

    #[test]
    fn an_unguarded_split_over_a_null_column_drops_the_attribute_keeps_the_entity_and_records_both() {
        let (value, dropped) = extract_dafif_code(SPLIT, "array", &JsonValue::Null);

        assert_eq!(value, None);
        let entries = BatchSink::from(dropped.templates).into_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, name("dafifCode"));
        assert_eq!(entries[0].1.entity, urn("urn:ngsi-ld:Country:Bonaire"));
        assert_eq!(entries[0].1.occurrences.get(), 1);
    }

    #[test]
    fn a_json_decoded_object_column_becomes_the_object_it_encodes() {
        let (value, _) = extract_dafif_code("{{ this[2] | json_decode }}", "object", &json!(r#"{"a":1}"#));

        assert_eq!(value, Some(Value::try_from(json!({"a": 1})).unwrap()));
    }

    #[test]
    fn an_array_text_column_becomes_the_array_it_encodes_without_decoding_in_the_template() {
        let (value, _) = extract_dafif_code("{{ this[2] }}", "array", &json!(r#"["BS","IN"]"#));

        assert_eq!(value, Some(Value::try_from(json!(["BS", "IN"])).unwrap()));
    }

    #[test]
    fn an_object_text_column_becomes_the_object_it_encodes_without_decoding_in_the_template() {
        let (value, _) = extract_dafif_code("{{ this[2] }}", "object", &json!(r#"{"a":1}"#));

        assert_eq!(value, Some(Value::try_from(json!({"a": 1})).unwrap()));
    }

    #[test]
    fn a_json_text_column_becomes_the_number_it_encodes() {
        let (value, _) = extract_dafif_code("{{ this[2] }}", "json", &json!("12.5"));

        assert_eq!(value, Some(Value::try_from(12.5).unwrap()));
    }

    #[test]
    fn a_column_that_is_not_json_under_the_json_transformation_drops_the_attribute_and_is_recorded() {
        let (value, dropped) = extract_dafif_code("{{ this[2] }}", "json", &json!("BS IN"));

        assert_eq!(value, None);
        let recorded = BatchSink::from(dropped.unusable_values)
            .into_entries()
            .map(|(unusable, record)| (unusable.attribute.to_string(), unusable.required, record.example.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(recorded, [("dafifCode".to_owned(), RequiredValue::Json, "BS IN".to_owned())]);
    }

    #[test]
    fn json_and_list_properties_read_csv_text_as_the_json_it_encodes() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Stop",
                identity: { entityName: "{{ id }}" },
                attributes: {
                    pollutantIndex: { type: "JsonProperty", source: "{{ indices }}", transformation: "json" },
                    nextCountdowns: { type: "ListProperty", source: "{{ countdowns }}" },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let mapped = AssembledEntity::from_single(
            entity(
                "urn:ngsi-ld:Stop:A",
                json!({"id": "A", "indices": r#"{"no2":3,"pm10":null}"#, "countdowns": "[1,7,15]"}),
            ),
            mapping,
        );
        let dropped = DroppedAttributes::new();

        let (result, _) = extractor.extract(mapped, &dropped).unwrap().into_parts();

        let values = result.values().as_ref().expect("values set");
        let Some(ResolvedAttribute::Single(AttributeContent::Json(payload))) = values.get(&name("pollutantIndex")).cloned() else {
            panic!("raw JSON");
        };
        assert_eq!(JsonValue::from(payload), json!({"no2": 3, "pm10": null}));
        assert_eq!(
            single_value(values.get(&name("nextCountdowns")).cloned()),
            Some(Value::try_from(json!([1, 7, 15])).unwrap())
        );
        assert!(dropped.nulls.is_empty());
    }

    /// Extracts the `label` attribute that the JSON5 attribute `declaration` builds over one record.
    /// Extraction itself must succeed: the entity is produced.
    fn extract_label(declaration: &str, record: JsonValue) -> Option<Value> {
        extract_label_recording(declaration, record, &DroppedAttributes::new())
    }

    /// Extracts the `label` attribute that the JSON5 attribute `declaration` builds over one record,
    /// recording every attribute the extraction drops into `dropped`.
    fn extract_label_recording(declaration: &str, record: JsonValue, dropped: &DroppedAttributes) -> Option<Value> {
        single_value(resolve_label(declaration, record, dropped))
    }

    /// Resolves the `label` attribute that the JSON5 attribute `declaration` builds over one record,
    /// recording every loss into `dropped`.
    fn resolve_label(declaration: &str, record: JsonValue, dropped: &DroppedAttributes) -> Option<ResolvedAttribute> {
        let (resolver, mapping) = prepare(&format!(
            r#"{{
                version: "v4",
                dataModel: "Item",
                identity: {{ entityName: "Item-1" }},
                attributes: {{ label: {declaration} }},
            }}"#
        ));
        let extractor = EntityExtractor::new(resolver);

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Item:1", record), mapping);
        let (result, _) = extractor.extract(mapped, dropped).unwrap().into_parts();

        result.values().as_ref().expect("values set").get(&name("label")).cloned()
    }

    #[test]
    fn an_unconverted_split_expression_becomes_the_compact_json_text_of_its_tokens() {
        let value = extract_label(r#"{ source: "{{ codes | split(pat=' ') }}" }"#, json!({"codes": "BS IN"}));

        assert_eq!(value, Some(Value::String(r#"["BS","IN"]"#.into())));
    }

    #[test]
    fn a_split_expression_under_a_string_transformation_becomes_the_compact_json_text_of_its_tokens() {
        let value = extract_label(
            r#"{ source: "{{ codes | split(pat=' ') }}", transformation: "string" }"#,
            json!({"codes": "BS IN"}),
        );

        assert_eq!(value, Some(Value::String(r#"["BS","IN"]"#.into())));
    }

    #[test]
    fn an_unconverted_json_decoded_object_becomes_its_compact_json_text() {
        let value = extract_label(r#"{ source: "{{ payload | json_decode }}" }"#, json!({"payload": r#"{"a":1}"#}));

        assert_eq!(value, Some(Value::String(r#"{"a":1}"#.into())));
    }

    #[test]
    fn an_unconverted_rounded_expression_renders_like_an_unconverted_float_field() {
        let rounded = extract_label(r#"{ source: "{{ n | round }}" }"#, json!({"n": 2.6}));
        let field = extract_label(r#"{ source: "{{ n }}" }"#, json!({"n": 3.0}));

        assert_eq!(rounded, Some(Value::String("3".into())));
        assert_eq!(field, rounded);
    }

    #[test]
    fn a_loop_source_renders_every_iteration_to_text() {
        let value = extract_label(r#"{ source: "{% for c in codes %}{{ c }} {% endfor %}" }"#, json!({"codes": ["a", "b"]}));

        assert_eq!(value, Some(Value::String("a b ".into())));
    }

    #[test]
    fn an_unfiltered_arithmetic_source_under_an_integer_transformation_yields_its_number() {
        let value = extract_label(r#"{ source: "{{ n + 1 }}", transformation: "integer" }"#, json!({"n": 2}));

        assert_eq!(value, Some(Value::try_from(json!(3)).unwrap()));
    }

    #[test]
    fn a_whole_record_reference_written_without_spaces_binds_the_whole_record() {
        let value = extract_label(r#"{ source: "{{context}}", transformation: "object" }"#, json!({"a": 1, "b": "x"}));

        assert_eq!(value, Some(Value::try_from(json!({"a": 1, "b": "x"})).unwrap()));
    }

    #[test]
    fn a_failed_relationship_property_drops_the_relationship_with_it() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Vehicle",
                identity: { entityName: "Vehicle-{{ id }}" },
                attributes: {
                    reference: { source: "{{ id }}" },
                    owner: {
                        type: "Relationship",
                        source: "{{ owner }}",
                        target: { entity: "Person" },
                        properties: { since: { source: "{{ since | upper }}" } },
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let dropped = DroppedAttributes::new();
        let mut relationships = Relationships::default();
        relationships.insert(name("owner"), vec![urn("urn:ngsi-ld:Person:7")]);
        let source = Entity::new(urn("urn:ngsi-ld:Vehicle:1"), json!({"id": "1", "owner": "7"}), None, None, relationships, None);

        let (result, _) = extractor.extract(AssembledEntity::from_single(source, mapping), &dropped).unwrap().into_parts();

        assert!(!result.relationships().contains_key(&name("owner")));
        assert!(result.values().as_ref().expect("values set").contains_key(&name("reference")));
        assert_eq!(BatchSink::from(dropped.templates).into_entries().collect::<Vec<_>>()[0].0, name("owner"));
    }

    #[test]
    fn a_field_array_source_is_kept_as_it_is_under_an_array_transformation() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Product",
                identity: { entityName: "Product-{{ id }}" },
                attributes: { labels: { source: "{{ labels_tags }}", transformation: "array" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let record = json!({"id": "1", "labels_tags": ["en:organic", "", "en:vegan"]});

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Product:1", record), mapping);
        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();

        assert_eq!(
            result.values().as_ref().expect("values set").get(&name("labels")),
            Some(&single(Value::try_from(json!(["en:organic", "", "en:vegan"])).unwrap()))
        );
    }

    #[test]
    fn a_multi_part_source_collects_one_element_per_part_under_an_array_transformation() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Product",
                identity: { entityName: "Product-{{ id }}" },
                attributes: { codes: { source: ["{{ a }}", "{{ b }}"], transformation: "array" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Product:1", json!({"id": "1", "a": "X", "b": 2})), mapping);
        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();

        assert_eq!(
            result.values().as_ref().expect("values set").get(&name("codes")),
            Some(&single(Value::try_from(json!(["X", 2])).unwrap()))
        );
    }

    #[test]
    fn a_list_property_without_a_transformation_takes_a_field_array_as_its_list() {
        let value = extract_label(r#"{ type: "ListProperty", source: "{{ codes }}" }"#, json!({"codes": ["BS", "IN"]}));

        assert_eq!(value, Some(Value::try_from(json!(["BS", "IN"])).unwrap()));
    }

    #[test]
    fn a_list_property_without_a_transformation_wraps_a_scalar_field_as_a_one_element_list() {
        let value = extract_label(r#"{ type: "ListProperty", source: "{{ codes }}" }"#, json!({"codes": "BS"}));

        assert_eq!(value, Some(Value::try_from(json!(["BS"])).unwrap()));
    }

    #[test]
    fn a_list_property_without_a_transformation_over_a_blank_or_null_field_is_absent() {
        for codes in [json!(""), JsonValue::Null] {
            let value = extract_label(r#"{ type: "ListProperty", source: "{{ codes }}" }"#, json!({"codes": codes}));

            assert_eq!(value, None);
        }
    }

    /// The raw JSON a `JsonProperty` declaration resolves to over one record, with the losses
    /// recorded.
    fn resolve_json_label(declaration: &str, record: JsonValue) -> (Option<JsonValue>, DroppedAttributes) {
        let dropped = DroppedAttributes::new();
        let payload = match resolve_label(declaration, record, &dropped) {
            None => None,
            Some(ResolvedAttribute::Single(AttributeContent::Json(payload))) => Some(JsonValue::from(payload)),
            Some(
                other @ (ResolvedAttribute::Single(AttributeContent::Value(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_))
                | ResolvedAttribute::Instances(_)),
            ) => {
                panic!("expected raw JSON, got {other:?}")
            }
        };
        (payload, dropped)
    }

    #[test]
    fn a_json_property_without_a_transformation_keeps_a_field_object_as_it_is() {
        let (payload, _) = resolve_json_label(r#"{ type: "JsonProperty", source: "{{ payload }}" }"#, json!({"payload": {"a": 1, "b": ["x"]}}));

        assert_eq!(payload, Some(json!({"a": 1, "b": ["x"]})));
    }

    #[test]
    fn a_json_property_without_a_transformation_keeps_a_field_array_as_it_is() {
        let (payload, _) = resolve_json_label(r#"{ type: "JsonProperty", source: "{{ payload }}" }"#, json!({"payload": [{"a": 1}, {"b": 2}]}));

        assert_eq!(payload, Some(json!([{"a": 1}, {"b": 2}])));
    }

    #[test]
    fn a_json_property_keeps_a_null_nested_in_its_source_and_records_no_loss() {
        let (payload, dropped) = resolve_json_label(r#"{ type: "JsonProperty", source: "{{ payload }}" }"#, json!({"payload": {"maybe": null}}));

        assert_eq!(payload, Some(json!({"maybe": null})));
        assert!(dropped.nulls.is_empty());
    }

    #[test]
    fn a_json_property_over_a_scalar_field_is_dropped_and_its_refusal_recorded() {
        let (payload, dropped) = resolve_json_label(r#"{ type: "JsonProperty", source: "{{ payload }}" }"#, json!({"payload": 3.5}));

        assert_eq!(payload, None);
        let entries = BatchSink::from(dropped.json_payloads).into_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.attribute, name("label"));
        assert_eq!(entries[0].0.refusal, JsonPayloadError::NotAnObject { found: JsonShape::Number });
    }

    #[test]
    fn a_json_property_under_an_object_transformation_carries_the_object_with_its_nulls_and_records_no_loss() {
        let (payload, dropped) = resolve_json_label(
            r#"{ type: "JsonProperty", source: "{{ payload }}", transformation: "object" }"#,
            json!({"payload": {"a": 1, "b": null}}),
        );

        assert_eq!(payload, Some(json!({"a": 1, "b": null})));
        assert!(dropped.nulls.is_empty());
    }

    #[test]
    fn an_object_of_only_nulls_on_a_property_is_dropped_and_recorded_without_a_dropped_null() {
        let dropped = DroppedAttributes::new();
        let value = extract_label_recording(
            r#"{ source: "{{ reading }}", transformation: "object" }"#,
            json!({"reading": {"maybe": null}}),
            &dropped,
        );

        assert_eq!(value, None);
        assert!(dropped.nulls.is_empty());
        let recorded = BatchSink::from(dropped.unusable_values)
            .into_entries()
            .map(|(unusable, record)| (unusable.attribute.to_string(), unusable.required, record.example.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(recorded, [("label".to_owned(), RequiredValue::NonNull, r#"{"maybe":null}"#.to_owned())]);
    }

    #[test]
    fn a_property_assembled_from_mappings_leaves_out_a_member_s_nulls_under_the_attribute() {
        let dropped = DroppedAttributes::new();
        let value = extract_label_recording(
            r#"{ mappings: { info: { source: "{{ info }}", transformation: "object" } } }"#,
            json!({"info": {"a": null, "b": 1}}),
            &dropped,
        );

        assert_eq!(value, Some(Value::try_from(json!({"info": {"b": 1}})).unwrap()));
        let entries = BatchSink::from(dropped.nulls)
            .into_entries()
            .map(|(dropped, record)| (dropped, record.occurrences().get()))
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            [(
                DroppedNull {
                    attribute: name("label"),
                    site: NullSite::ObjectMember,
                },
                1
            )]
        );
    }

    #[test]
    fn a_json_property_assembled_from_mappings_keeps_a_member_s_nulls() {
        let (payload, dropped) = resolve_json_label(
            r#"{ type: "JsonProperty", mappings: { info: { source: "{{ info }}", transformation: "object" } } }"#,
            json!({"info": {"a": null}}),
        );

        assert_eq!(payload, Some(json!({"info": {"a": null}})));
        assert!(dropped.nulls.is_empty());
        assert!(dropped.unusable_values.is_empty());
    }

    #[test]
    fn a_datetime_member_of_an_assembled_object_is_written_as_its_text() {
        let value = extract_label(
            r#"{ mappings: { at: { source: "{{ at }}", transformation: "datetime" } } }"#,
            json!({"at": "2026-03-01 11:04:35"}),
        );

        assert_eq!(serde_json::to_value(value).unwrap(), json!({"at": "2026-03-01T11:04:35Z"}));
    }

    #[test]
    fn a_geometry_member_of_an_assembled_object_is_written_as_its_geojson() {
        let value = extract_label(
            r#"{ mappings: { pin: { type: "GeoProperty", source: ["{{ lon }}", "{{ lat }}"], transformation: "point" } } }"#,
            json!({"lon": 16.37, "lat": 48.2}),
        );

        assert_eq!(
            serde_json::to_value(value).unwrap(),
            json!({"pin": {"type": "Point", "coordinates": [16.37, 48.2]}})
        );
    }

    #[test]
    fn a_list_property_over_a_source_array_holding_a_null_drops_the_null_and_records_it() {
        let dropped = DroppedAttributes::new();
        let value = extract_label_recording(r#"{ type: "ListProperty", source: "{{ tags }}" }"#, json!({"tags": ["a", null, "b"]}), &dropped);

        assert_eq!(value, Some(Value::try_from(json!(["a", "b"])).unwrap()));
        let entries = BatchSink::from(dropped.nulls)
            .into_entries()
            .map(|(dropped, record)| (dropped, record.occurrences().get()))
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            [(
                DroppedNull {
                    attribute: name("label"),
                    site: NullSite::ArrayItem,
                },
                1
            )]
        );
    }

    #[test]
    fn an_object_property_over_a_source_object_holding_a_null_member_drops_the_member_and_records_it() {
        let dropped = DroppedAttributes::new();
        let value = extract_label_recording(
            r#"{ source: "{{ reading }}", transformation: "object" }"#,
            json!({"reading": {"value": 21.5, "unit": null}}),
            &dropped,
        );

        assert_eq!(value, Some(Value::try_from(json!({"value": 21.5})).unwrap()));
        let entries = BatchSink::from(dropped.nulls)
            .into_entries()
            .map(|(dropped, record)| (dropped, record.occurrences().get()))
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            [(
                DroppedNull {
                    attribute: name("label"),
                    site: NullSite::ObjectMember,
                },
                1
            )]
        );
    }

    #[test]
    fn a_property_over_an_empty_string_field_keeps_the_empty_string() {
        let value = extract_label(r#"{ source: "{{ label }}" }"#, json!({"label": ""}));

        assert_eq!(value, Some(Value::String("".into())));
    }

    #[test]
    fn a_property_without_a_transformation_still_turns_a_field_array_into_compact_json_text() {
        let value = extract_label(r#"{ source: "{{ codes }}" }"#, json!({"codes": ["BS", "IN"]}));

        assert_eq!(value, Some(Value::String(r#"["BS","IN"]"#.into())));
    }

    /// The `Point` geometry at `coordinates`, as the extraction stage types it.
    fn point(coordinates: [f64; 2]) -> Value {
        Value::Geospatial(Box::new(NgsiLdGeometry::Point {
            coordinates: coordinates.into(),
        }))
    }

    #[test]
    fn a_geo_property_without_a_transformation_keeps_a_geojson_point_field_as_its_geometry() {
        let dropped = DroppedAttributes::new();
        let value = extract_label_recording(
            r#"{ type: "GeoProperty", source: "{{ geometry }}" }"#,
            json!({"geometry": {"type": "Point", "coordinates": [14.5, 46.05]}}),
            &dropped,
        );

        assert_eq!(value, Some(point([14.5, 46.05])));
        assert!(dropped.geometries.is_empty());
    }

    #[test]
    fn a_geo_property_without_a_transformation_reads_a_geojson_geometry_written_as_text() {
        let value = extract_label(
            r#"{ type: "GeoProperty", source: "{{ geometry }}" }"#,
            json!({"geometry": r#"{"type":"Point","coordinates":[14.5,46.05]}"#}),
        );

        assert_eq!(value, Some(point([14.5, 46.05])));
    }

    #[test]
    fn a_geo_property_over_a_value_that_is_no_geometry_is_dropped_and_recorded_with_or_without_a_transformation() {
        for declaration in [
            r#"{ type: "GeoProperty", source: "{{ geometry }}" }"#,
            r#"{ type: "GeoProperty", source: "{{ geometry }}", transformation: "geometry" }"#,
        ] {
            for (geometry, quoted) in [
                (json!("somewhere near the river"), "somewhere near the river"),
                (json!({"type": "Point"}), r#"{"type":"Point"}"#),
                (json!(42), "42"),
            ] {
                let (value, recorded) = extract_label_with_unusable(declaration, json!({"geometry": geometry}));

                assert_eq!(value, None, "{declaration} {quoted}");
                assert_eq!(recorded, [("label".to_owned(), RequiredValue::Geometry, quoted.to_owned())], "{declaration}");
            }
        }
    }

    #[test]
    fn a_geo_property_without_a_transformation_over_a_missing_null_or_blank_field_is_absent_without_a_refusal() {
        for record in [json!({}), json!({"geometry": null}), json!({"geometry": ""}), json!({"geometry": "  "})] {
            let (value, recorded) = extract_label_with_unusable(r#"{ type: "GeoProperty", source: "{{ geometry }}" }"#, record);

            assert_eq!(value, None);
            assert!(recorded.is_empty(), "{recorded:?}");
        }
    }

    #[test]
    fn a_geo_property_without_a_transformation_loads_and_applies_its_geometry_block() {
        let dropped = DroppedAttributes::new();
        let value = extract_label_recording(
            r#"{ type: "GeoProperty", source: "{{ geometry }}", geometry: { convert: "centroid" } }"#,
            json!({"geometry": {"type": "Polygon", "coordinates": [[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [0.0, 0.0]]]}}),
            &dropped,
        );

        assert_eq!(value, Some(point([1.0, 1.0])));
        assert!(dropped.geometries.is_empty());
    }

    #[test]
    fn a_geometry_collection_under_a_geo_property_without_a_transformation_is_refused_and_recorded() {
        let dropped = DroppedAttributes::new();
        let collection = json!({
            "type": "GeometryCollection",
            "geometries": [
                {"type": "Point", "coordinates": [1.0, 2.0]},
                {"type": "Point", "coordinates": [3.0, 4.0]},
            ],
        });

        let value = extract_label_recording(
            r#"{ type: "GeoProperty", source: "{{ geometry }}" }"#,
            json!({"geometry": collection}),
            &dropped,
        );

        assert_eq!(value, None);
        let entries = BatchSink::from(dropped.geometries).into_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.attribute, name("label"));
    }

    #[test]
    fn a_geometry_collection_under_a_geo_property_without_a_transformation_folds_when_the_geometry_block_says_so() {
        let collection = json!({
            "type": "GeometryCollection",
            "geometries": [
                {"type": "Point", "coordinates": [1.0, 2.0]},
                {"type": "Point", "coordinates": [3.0, 4.0]},
            ],
        });

        let value = extract_label(
            r#"{ type: "GeoProperty", source: "{{ geometry }}", geometry: { convert: "flatten" } }"#,
            json!({"geometry": collection}),
        );

        assert_eq!(
            value,
            Some(Value::Geospatial(Box::new(NgsiLdGeometry::MultiPoint {
                coordinates: vec![[1.0, 2.0].into(), [3.0, 4.0].into()],
            })))
        );
    }

    /// A polygon whose exterior ring is wound clockwise, against RFC 7946 clause 3.1.6's right-hand
    /// rule.
    fn clockwise_square() -> JsonValue {
        json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [0.0, 2.0], [2.0, 2.0], [2.0, 0.0], [0.0, 0.0]]]})
    }

    /// The `footprint` sub-attribute of `name` that the JSON5 `footprint` declaration builds over
    /// one record, recording every attribute the extraction drops into `dropped`.
    fn extract_footprint(footprint: &str, record: JsonValue, dropped: &DroppedAttributes) -> Option<SubAttribute> {
        let (resolver, mapping) = prepare(&format!(
            r#"{{
                version: "v4",
                dataModel: "Zone",
                identity: {{ entityName: "Zone-1" }},
                attributes: {{ name: {{ source: "{{{{ id }}}}", properties: {{ footprint: {footprint} }} }} }},
            }}"#
        ));
        let extractor = EntityExtractor::new(resolver);

        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Zone:1", record), mapping);
        let (result, _) = extractor.extract(mapped, dropped).unwrap().into_parts();

        result
            .metadata()
            .as_ref()
            .and_then(|metadata| metadata.get(&name("name")))
            .and_then(MetadataStorage::as_shared)
            .and_then(|shared| shared.get(&name("footprint")))
            .cloned()
    }

    #[test]
    fn a_geo_property_sub_attribute_is_carried_as_the_geometry_its_own_geometry_block_produced() {
        let dropped = DroppedAttributes::new();
        let footprint = extract_footprint(
            r#"{ type: "GeoProperty", source: "{{ geometry }}", geometry: { winding: "keep" } }"#,
            json!({"id": "1", "geometry": clockwise_square()}),
            &dropped,
        )
        .expect("footprint sub-attribute");

        assert_eq!(*footprint.kind(), NgsiLdAttributeKind::GeoProperty);
        let expected: NgsiLdGeometry = serde_json::from_value(clockwise_square()).unwrap();
        assert_eq!(footprint.value(), &AttributeContent::Value(Value::Geospatial(Box::new(expected))));
        assert!(dropped.geometries.is_empty());
    }

    #[test]
    fn a_geometry_collection_under_a_geo_property_sub_attribute_is_refused_and_recorded_under_its_name() {
        let dropped = DroppedAttributes::new();
        let collection = json!({"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [1.0, 2.0]}]});

        let footprint = extract_footprint(
            r#"{ type: "GeoProperty", source: "{{ geometry }}" }"#,
            json!({"id": "1", "geometry": collection}),
            &dropped,
        );

        assert_eq!(footprint, None);
        let entries = BatchSink::from(dropped.geometries).into_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.attribute, name("footprint"));
    }

    #[test]
    fn a_geo_property_sub_attribute_over_a_value_that_is_no_geometry_is_dropped_on_its_own_and_recorded_under_its_path() {
        let dropped = DroppedAttributes::new();
        let footprint = extract_footprint(
            r#"{ type: "GeoProperty", source: "{{ geometry }}" }"#,
            json!({"id": "1", "geometry": {"type": "Point"}}),
            &dropped,
        );

        assert_eq!(footprint, None);
        let recorded = BatchSink::from(dropped.unusable_values)
            .into_entries()
            .map(|(unusable, record)| (unusable.attribute.to_string(), unusable.required, record.example.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(
            recorded,
            [("name.footprint".to_owned(), RequiredValue::Geometry, r#"{"type":"Point"}"#.to_owned())]
        );
    }

    #[test]
    fn a_geo_property_assembled_from_nested_mappings_is_converted_under_its_own_geometry_block() {
        let dropped = DroppedAttributes::new();
        let value = extract_label_recording(
            r#"{
                type: "GeoProperty",
                geometry: { winding: "keep" },
                mappings: {
                    type: { source: "Polygon" },
                    coordinates: { source: "{{ rings }}", transformation: "array" },
                },
            }"#,
            json!({"rings": [[[0.0, 0.0], [0.0, 2.0], [2.0, 2.0], [2.0, 0.0], [0.0, 0.0]]]}),
            &dropped,
        );

        let expected: NgsiLdGeometry = serde_json::from_value(clockwise_square()).unwrap();
        assert_eq!(value, Some(Value::Geospatial(Box::new(expected))));
        assert!(dropped.geometries.is_empty());
    }

    #[test]
    fn a_geometry_collection_assembled_from_nested_mappings_under_a_geo_property_is_refused_and_recorded() {
        let dropped = DroppedAttributes::new();
        let value = extract_label_recording(
            r#"{
                type: "GeoProperty",
                mappings: {
                    type: { source: "GeometryCollection" },
                    geometries: { source: "{{ members }}", transformation: "array" },
                },
            }"#,
            json!({"members": [{"type": "Point", "coordinates": [1.0, 2.0]}]}),
            &dropped,
        );

        assert_eq!(value, None);
        let entries = BatchSink::from(dropped.geometries).into_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.attribute, name("label"));
    }

    #[test]
    fn an_object_assembled_from_nested_mappings_under_a_conversion_reading_no_geometry_is_kept_as_assembled() {
        let value = extract_label(
            r#"{ transformation: "object", mappings: { street: { source: "{{ street }}" } } }"#,
            json!({"street": "Trubarjeva"}),
        );

        assert_eq!(value, Some(Value::try_from(json!({"street": "Trubarjeva"})).unwrap()));
    }

    #[test]
    fn an_object_assembled_from_nested_mappings_without_a_transformation_is_kept_as_assembled() {
        let value = extract_label(
            r#"{ mappings: { street: { source: "{{ street }}" }, city: { source: "{{ city }}" } } }"#,
            json!({"street": "Ring", "city": "Wien"}),
        );

        assert_eq!(value, Some(Value::try_from(json!({"street": "Ring", "city": "Wien"})).unwrap()));
    }

    #[test]
    fn an_object_assembled_from_nested_mappings_under_a_string_transformation_becomes_its_compact_json_text() {
        let value = extract_label(
            r#"{ transformation: "string", mappings: { street: { source: "{{ street }}" }, city: { source: "{{ city }}" } } }"#,
            json!({"street": "Ring", "city": "Wien"}),
        );

        assert_eq!(value, Some(Value::String(r#"{"street":"Ring","city":"Wien"}"#.into())));
    }

    #[test]
    fn an_object_assembled_from_nested_mappings_under_an_array_transformation_becomes_a_list_of_the_object() {
        let value = extract_label(
            r#"{ transformation: "array", mappings: { street: { source: "{{ street }}" } } }"#,
            json!({"street": "Ring"}),
        );

        assert_eq!(value, Some(Value::try_from(json!([{"street": "Ring"}])).unwrap()));
    }

    #[test]
    fn an_object_assembled_from_nested_mappings_under_the_json_transformation_is_kept_as_assembled() {
        let value = extract_label(
            r#"{ transformation: "json", mappings: { street: { source: "{{ street }}" } } }"#,
            json!({"street": "Ring"}),
        );

        assert_eq!(value, Some(Value::try_from(json!({"street": "Ring"})).unwrap()));
    }

    #[test]
    fn a_point_assembled_from_nested_mappings_under_a_multipoint_transformation_becomes_a_multipoint() {
        let value = extract_label(
            r#"{
                transformation: "multipoint",
                mappings: { type: { source: "Point" }, coordinates: { source: ["{{ lon }}", "{{ lat }}"], transformation: "array" } },
            }"#,
            json!({"lon": 16.37, "lat": 48.2}),
        );

        let expected: NgsiLdGeometry = serde_json::from_value(json!({"type": "MultiPoint", "coordinates": [[16.37, 48.2]]})).unwrap();
        assert_eq!(value, Some(Value::Geospatial(Box::new(expected))));
    }

    /// Extracts `label` over one record and returns it with every unusable value the extraction
    /// recorded, as the attribute path, the requirement, and the first value met.
    fn extract_label_with_unusable(declaration: &str, record: JsonValue) -> (Option<Value>, Vec<(String, RequiredValue, String)>) {
        let dropped = DroppedAttributes::new();
        let value = extract_label_recording(declaration, record, &dropped);
        let recorded = BatchSink::from(dropped.unusable_values)
            .into_entries()
            .map(|(unusable, record)| (unusable.attribute.to_string(), unusable.required, record.example.to_string()))
            .collect();

        (value, recorded)
    }

    #[test]
    fn an_object_assembled_from_nested_mappings_that_spells_no_geometry_under_a_point_transformation_is_dropped_and_recorded() {
        let (value, recorded) = extract_label_with_unusable(
            r#"{ transformation: "point", mappings: { street: { source: "{{ street }}" } } }"#,
            json!({"street": "Ring"}),
        );

        assert_eq!(value, None);
        assert_eq!(recorded, [("label".to_owned(), RequiredValue::Geometry, r#"{"street":"Ring"}"#.to_owned())]);
    }

    #[test]
    fn an_object_assembled_from_nested_mappings_that_spells_no_geometry_under_a_geo_property_is_dropped_and_recorded() {
        let (value, recorded) = extract_label_with_unusable(
            r#"{ type: "GeoProperty", mappings: { street: { source: "{{ street }}" } } }"#,
            json!({"street": "Ring"}),
        );

        assert_eq!(value, None);
        assert_eq!(recorded, [("label".to_owned(), RequiredValue::Geometry, r#"{"street":"Ring"}"#.to_owned())]);
    }

    #[test]
    fn list_property_instances_without_a_transformation_each_take_their_field_array_as_their_list() {
        let resolved = resolve_label(
            r#"{
                type: "ListProperty",
                instances: [
                    { source: "{{ a }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:a" } } },
                    { source: "{{ b }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:b" } } },
                ],
            }"#,
            json!({"a": ["x", "y"], "b": "z"}),
            &DroppedAttributes::new(),
        );

        assert_eq!(
            resolved,
            Some(ResolvedAttribute::Instances(vec![
                Some(AttributeContent::Value(Value::try_from(json!(["x", "y"])).unwrap())),
                Some(AttributeContent::Value(Value::try_from(json!(["z"])).unwrap())),
            ]))
        );
    }

    #[test]
    fn a_list_property_sub_attribute_without_a_transformation_takes_a_field_array_as_its_list() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Product",
                identity: { entityName: "Product-{{ id }}" },
                attributes: {
                    sugars: {
                        source: "{{ sugars }}",
                        transformation: "float",
                        properties: { codes: { type: "ListProperty", source: "{{ codes }}" } },
                    },
                },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let record = json!({"id": "1", "sugars": 12.0, "codes": ["BS", "IN"]});
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Product:1", record), mapping);

        let (result, _) = extractor.extract(mapped, &DroppedAttributes::new()).unwrap().into_parts();
        let metadata = result.metadata().as_ref().expect("metadata set");
        let shared = metadata.get(&name("sugars")).expect("sugars metadata").as_shared().expect("shared metadata");
        let codes = shared.get(&name("codes")).expect("codes sub-attribute");

        assert_eq!(*codes.kind(), NgsiLdAttributeKind::ListProperty);
        assert_eq!(codes.value(), &AttributeContent::Value(Value::try_from(json!(["BS", "IN"])).unwrap()));
    }

    #[test]
    fn a_lone_non_ascii_field_reference_reads_its_column() {
        let value = extract_label(r#"{ source: "{{ čas }}" }"#, json!({"čas": "10:00"}));

        assert_eq!(value, Some(Value::String("10:00".into())));
    }

    #[test]
    fn a_filter_over_a_null_field_records_a_failure_naming_the_field_and_the_declaration() {
        let (resolver, mapping) = prepare(
            r#"{
                version: "v4",
                dataModel: "Item",
                identity: { entityName: "Item-1" },
                attributes: { codes: { source: "{{ code | split(pat=' ') }}" } },
            }"#,
        );
        let extractor = EntityExtractor::new(resolver);
        let dropped = DroppedAttributes::new();
        let mapped = AssembledEntity::from_single(entity("urn:ngsi-ld:Item:1", json!({"code": null})), mapping);

        extractor.extract(mapped, &dropped).unwrap();

        let entries = BatchSink::from(dropped.templates).into_entries().collect::<Vec<_>>();
        let error = &entries[0].1.error;
        assert_eq!(error.location.site, TemplateSite::Attribute(name("codes")));
        assert_eq!(*error.location.document, *Path::new("test.json5"));
        assert!(matches!(error.failure.as_ref(), ResolutionFailure::NullField { field, .. } if field.as_str() == "code"));
    }
}
