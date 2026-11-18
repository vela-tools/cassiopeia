use crate::{
    attribute::{instance_selection::InstanceSelection, resolution_context::ResolutionContext, resolution_error::Result, resolver::resolve},
    dropped_attributes::DroppedAttributes,
};
use cassiopeia_ir::{
    assembled_entity::AssembledFragment,
    attribute_content::AttributeContent,
    entity_relationships::EntityRelationships,
    metadata::{EntityMetadata, MetadataStorage},
    observed_records::RecordChoice,
    resolved_attribute::ResolvedAttribute,
    sub_attribute::SubAttributes,
};
use cassiopeia_mapping::{
    attribute::Attribute,
    observation_slot::AttributeSlots,
    template::{resolver::TemplateResolver, template_input::TemplateInput},
};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use smallvec::SmallVec;
use std::{iter, mem};

/// A fragment whose mapping kept its attributes from several records, with what resolving it
/// against any of them reads.
///
/// ETSI GS CIM 009 v1.9.1 clause 4.5.5.3 resolves conflicting instances of an attribute per
/// attribute, so a current-state store kept each attribute, and each instance, from the record
/// where it was observed last. Each resolves here against that record, and an attribute whose
/// instances were kept from several records renders each record's instances against it alone.
pub(crate) struct ObservedFragment<'a> {
    /// The fragment, carrying its newest record and the records its slots kept besides.
    pub(crate) fragment: &'a AssembledFragment,
    /// The compiled-template resolver shared across the extraction stage.
    pub(crate) resolver: &'a TemplateResolver,
    /// Relationship objects already minted for the entity.
    pub(crate) relationships: &'a EntityRelationships,
    /// Where a refused conversion is recorded.
    pub(crate) dropped: &'a DroppedAttributes,
}

impl ObservedFragment<'_> {
    /// Resolves the top-level attribute `name`, declared as `config` at `index` among its mapping's
    /// attributes, recording its metadata in `metadata`.
    pub(crate) fn resolve(&self, index: usize, name: &NameBuf, config: &Attribute, metadata: &mut EntityMetadata) -> Result<Option<ResolvedAttribute>> {
        let slots = self.fragment.mapping().observation_plan().slots_of(index);
        let Some(instances) = config.instances() else {
            let choice = self.fragment.choice(slots.and_then(AttributeSlots::declaration));
            return self.resolve_on(choice, InstanceSelection::All, name, config, metadata);
        };
        let choices: SmallVec<[RecordChoice; 4]> = (0..instances.len())
            .map(|instance| self.fragment.choice(slots.and_then(|slots| slots.instance(instance))))
            .collect();
        match choices.split_first() {
            Some((first, rest)) if rest.iter().any(|choice| choice != first) => self.resolve_instance_groups(&choices, name, config, metadata),
            Some((first, _)) => self.resolve_on(*first, InstanceSelection::All, name, config, metadata),
            None => self.resolve_on(RecordChoice::Newest, InstanceSelection::All, name, config, metadata),
        }
    }

    /// Resolves the attribute against the record `choice` names, rendering the instances `instances`
    /// selects.
    fn resolve_on(
        &self,
        choice: RecordChoice,
        instances: InstanceSelection<'_>,
        name: &NameBuf,
        config: &Attribute,
        metadata: &mut EntityMetadata,
    ) -> Result<Option<ResolvedAttribute>> {
        let input = TemplateInput::new(self.fragment.record_of(choice), self.fragment.variables());
        let mut context = ResolutionContext::new(input, self.resolver, self.relationships, self.dropped, Some(metadata)).selecting(instances);
        resolve(&mut context, name, config)
    }

    /// Resolves an attribute whose instances were kept from several records: each record renders the
    /// instances kept from it, contents and per-item metadata alike, and each instance takes its
    /// place by its declared position, as it would rendered from one record.
    fn resolve_instance_groups(
        &self,
        choices: &[RecordChoice],
        name: &NameBuf,
        config: &Attribute,
        metadata: &mut EntityMetadata,
    ) -> Result<Option<ResolvedAttribute>> {
        let mut contents: Vec<Option<AttributeContent>> = iter::repeat_with(|| None).take(choices.len()).collect();
        let mut per_item: Vec<SubAttributes> = iter::repeat_with(SubAttributes::default).take(choices.len()).collect();
        let mut recorded = false;
        let mut groups: SmallVec<[RecordChoice; 4]> = SmallVec::new();
        for choice in choices {
            if !groups.contains(choice) {
                groups.push(*choice);
            }
        }
        for group in groups {
            let selected: SmallVec<[usize; 4]> = choices
                .iter()
                .enumerate()
                .filter(|(_, choice)| **choice == group)
                .map(|(index, _)| index)
                .collect();
            let mut group_metadata = EntityMetadata::default();
            match self.resolve_on(group, InstanceSelection::Only(&selected), name, config, &mut group_metadata)? {
                Some(ResolvedAttribute::Instances(mut resolved)) => {
                    for index in &selected {
                        if let (Some(content), Some(slot)) = (resolved.get_mut(*index), contents.get_mut(*index)) {
                            *slot = content.take();
                        }
                    }
                }
                // An attribute declaring instances resolves to one entry per instance, or, as a
                // relationship whose objects travel on the entity, to none.
                Some(ResolvedAttribute::Single(_)) | None => {}
            }
            match group_metadata.swap_remove(name) {
                Some(MetadataStorage::PerItem(mut items)) => {
                    recorded = true;
                    for index in &selected {
                        if let (Some(item), Some(slot)) = (items.get_mut(*index), per_item.get_mut(*index)) {
                            *slot = mem::take(item);
                        }
                    }
                }
                Some(MetadataStorage::Shared(shared)) => {
                    recorded = true;
                    for index in &selected {
                        if let Some(slot) = per_item.get_mut(*index) {
                            // Shared metadata answers for each instance, so each takes its own copy.
                            slot.clone_from(&shared);
                        }
                    }
                }
                None => {}
            }
        }
        if recorded {
            metadata.insert(name.clone(), MetadataStorage::per_item(per_item));
        }
        Ok(contents.iter().any(Option::is_some).then_some(ResolvedAttribute::Instances(contents)))
    }
}

#[cfg(test)]
mod tests {
    use crate::{dropped_attributes::DroppedAttributes, entity_extractor::EntityExtractor, extractor::Extractor};
    use cassiopeia_expander::compiler::ExpanderCompiler;
    use cassiopeia_ir::{
        assembled_entity::{AssembledEntity, AssembledFragment},
        entity::Entity,
        entity_relationships::EntityRelationships,
        observed_records::{ObservedRecords, RecordChoice},
        resolved_attribute::ResolvedAttribute,
        sub_attribute::SubAttributes,
    };
    use cassiopeia_mapping::{
        mapping::Mapping,
        template::{run_variables::RunVariables, runner::TemplateRunner},
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use smallvec::smallvec;
    use std::{path::Path, sync::Arc};

    /// Two attributes, each observed at a time of its own.
    const READING_AND_STATUS: &str = r#"{
        version: "v4",
        dataModel: "Sensor",
        identity: { entityName: "{{ code }}" },
        attributes: {
            reading: { source: "{{ reading }}", transformation: "float", properties: { observedAt: { source: "{{ read_time }}" } } },
            status: { source: "{{ status }}", properties: { observedAt: { source: "{{ status_time }}" } } },
        },
    }"#;

    /// Two instances of one forecast, each observed at a time of its own and qualified by its own
    /// provider, sharing a unit code.
    const FORECASTS: &str = r#"{
        version: "v4",
        dataModel: "WeatherForecast",
        identity: { entityName: "{{ code }}" },
        attributes: {
            temperature: {
                transformation: "float",
                properties: { unitCode: { source: "{{ unit }}" } },
                instances: [
                    {
                        source: "{{ ecmwf }}",
                        properties: {
                            datasetId: { source: "urn:ngsi-ld:Dataset:ecmwf" },
                            provider: { source: "{{ ecmwf_provider }}" },
                            observedAt: { source: "{{ ta }}" },
                        },
                    },
                    {
                        source: "{{ gfs }}",
                        properties: {
                            datasetId: { source: "urn:ngsi-ld:Dataset:gfs" },
                            provider: { source: "{{ gfs_provider }}" },
                            observedAt: { source: "{{ tb }}" },
                        },
                    },
                ],
            },
        },
    }"#;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    /// Extracts the entity `newest` and `kept_for_first`, kept for the first slot of `document`,
    /// assemble into when the second slot kept `newest`.
    fn extract(document: &str, newest: JsonValue, kept_for_first: JsonValue, dropped: &DroppedAttributes) -> Entity {
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, Path::new("test.json5"), &mut runner).unwrap();
        let observed = ObservedRecords::new(vec![kept_for_first], vec![RecordChoice::Observed(0), RecordChoice::Newest]);
        let fragment = AssembledFragment::new(newest, Some(Box::new(observed)), Arc::new(mapping), RunVariables::none());
        let assembled = AssembledEntity::new(
            "urn:ngsi-ld:Sensor:S1".parse().unwrap(),
            None,
            None,
            EntityRelationships::default(),
            smallvec![fragment],
        );
        EntityExtractor::new(runner.resolver()).extract(assembled, dropped).unwrap().into_parts().0
    }

    /// Every sub-attribute of `metadata` with a plain value, by name, as JSON.
    fn qualifiers(metadata: &SubAttributes) -> Vec<(String, JsonValue)> {
        metadata
            .iter()
            .filter_map(|(key, sub)| sub.value().as_value().map(|value| (key.to_string(), serde_json::to_value(value).unwrap())))
            .collect()
    }

    /// The value of the single-instance attribute `attribute` of `entity`, as JSON.
    fn value(entity: &Entity, attribute: &str) -> JsonValue {
        match entity.values().as_ref().unwrap().get(&name(attribute)) {
            Some(ResolvedAttribute::Single(content)) => serde_json::to_value(content.as_value().unwrap()).unwrap(),
            other => panic!("expected one value of `{attribute}`, got {other:?}"),
        }
    }

    /// The shared qualifiers of `attribute` of `entity`.
    fn shared(entity: &Entity, attribute: &str) -> Vec<(String, JsonValue)> {
        qualifiers(entity.metadata().as_ref().unwrap().get(&name(attribute)).unwrap().as_shared().unwrap())
    }

    #[test]
    fn each_attribute_resolves_against_the_record_its_slot_kept() {
        let newest = json!({"code": "S1", "reading": 21.5, "read_time": "2026-03-01T10:00:00Z", "status": "ok", "status_time": "2026-03-01T12:00:00Z"});
        let earlier = json!({"code": "S1", "reading": 22.5, "read_time": "2026-03-01T11:00:00Z", "status": "warm", "status_time": "2026-03-01T09:00:00Z"});

        let entity = extract(READING_AND_STATUS, newest, earlier, &DroppedAttributes::new());

        assert_eq!(value(&entity, "reading"), json!(22.5));
        assert_eq!(shared(&entity, "reading"), [("observedAt".to_string(), json!("2026-03-01T11:00:00Z"))]);
        assert_eq!(value(&entity, "status"), json!("ok"));
        assert_eq!(shared(&entity, "status"), [("observedAt".to_string(), json!("2026-03-01T12:00:00Z"))]);
    }

    #[test]
    fn each_instance_resolves_its_value_and_metadata_against_the_record_its_slot_kept() {
        let newest = json!({
            "code": "S1", "unit": "FAH", "ta": "2026-03-01T10:00:00Z", "tb": "2026-03-01T12:00:00Z",
            "ecmwf": 3.0, "ecmwf_provider": "ecmwf-newest", "gfs": 4.0, "gfs_provider": "gfs-newest",
        });
        let earlier = json!({
            "code": "S1", "unit": "CEL", "ta": "2026-03-01T11:00:00Z", "tb": "2026-03-01T09:00:00Z",
            "ecmwf": 1.0, "ecmwf_provider": "ecmwf-earlier", "gfs": 2.0, "gfs_provider": "gfs-earlier",
        });

        let entity = extract(FORECASTS, newest, earlier, &DroppedAttributes::new());

        let Some(ResolvedAttribute::Instances(instances)) = entity.values().as_ref().unwrap().get(&name("temperature")) else {
            panic!("expected the instances of `temperature`");
        };
        let instances: Vec<JsonValue> = instances
            .iter()
            .map(|instance| serde_json::to_value(instance.as_ref().unwrap().as_value().unwrap()).unwrap())
            .collect();
        assert_eq!(instances, [json!(1.0), json!(4.0)]);
        let items: Vec<Vec<(String, JsonValue)>> = entity
            .metadata()
            .as_ref()
            .unwrap()
            .get(&name("temperature"))
            .unwrap()
            .as_per_item()
            .unwrap()
            .iter()
            .map(qualifiers)
            .collect();
        assert_eq!(
            items,
            [
                vec![
                    ("unitCode".to_string(), json!("CEL")),
                    ("datasetId".to_string(), json!("urn:ngsi-ld:Dataset:ecmwf")),
                    ("provider".to_string(), json!("ecmwf-earlier")),
                    ("observedAt".to_string(), json!("2026-03-01T11:00:00Z")),
                ],
                vec![
                    ("unitCode".to_string(), json!("FAH")),
                    ("datasetId".to_string(), json!("urn:ngsi-ld:Dataset:gfs")),
                    ("provider".to_string(), json!("gfs-newest")),
                    ("observedAt".to_string(), json!("2026-03-01T12:00:00Z")),
                ],
            ]
        );
    }

    #[test]
    fn an_instance_not_kept_from_a_record_is_not_read_off_it() {
        // Each record holds an unusable value only in the instance not kept from it.
        let newest = json!({"code": "S1", "ta": "2026-03-01T10:00:00Z", "tb": "2026-03-01T12:00:00Z", "ecmwf": "not a number", "gfs": 4.0});
        let earlier = json!({"code": "S1", "ta": "2026-03-01T11:00:00Z", "tb": "2026-03-01T09:00:00Z", "ecmwf": 1.0, "gfs": "not a number"});
        let dropped = DroppedAttributes::new();

        extract(FORECASTS, newest, earlier, &dropped);

        assert!(dropped.unusable_values.is_empty());
    }
}
