use crate::attribute::resolution_error::Result as ResolutionResult;
use cassiopeia_ir::{
    attribute_owners::{AttributeOwners, CarriedDeclaration},
    entity_relationships::EntityRelationships,
    metadata::EntityMetadata,
    resolved_attribute::ResolvedAttribute,
};
use cassiopeia_mapping::{attribute::Attribute, mapping::Mapping};
use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};
use std::sync::Arc;

/// What resolving one declaration did to the attribute it names, in an entity several mappings
/// carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Claim {
    /// The declaration gave the attribute a value, so the attribute is taken from it.
    Takes,
    /// The declaration gave the attribute nothing, so a mapping carried later may still give it one.
    Yields,
}

impl Claim {
    /// The claim of the declaration `config` of `name`, which resolved to `resolved` for an entity
    /// whose relationship objects are `relationships`.
    ///
    /// A Property-family declaration gives its attribute a value by resolving one. A relationship's
    /// objects are joined across every mapping that names them and travel on the entity rather than
    /// on one fragment, so a relationship declaration gives its attribute a value whenever the entity
    /// carries objects for it and the declaration's own properties resolved.
    pub(crate) fn of(resolved: &ResolutionResult<Option<ResolvedAttribute>>, name: &NameBuf, config: &Attribute, relationships: &EntityRelationships) -> Claim {
        match resolved {
            Ok(Some(_)) => Claim::Takes,
            Ok(None)
                if matches!(config.kind(), NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship) && relationships.has_objects(name) =>
            {
                Claim::Takes
            }
            Ok(None) | Err(_) => Claim::Yields,
        }
    }
}

/// Settles which of the mappings carrying one entity each attribute name is taken from.
///
/// The mappings are carried in the order of their ids, the order of the manifest, and an attribute
/// name several of them declare is taken whole from the first whose fragment gives it a value: its
/// value, the declaration it is built with, and its sub-attributes. This is the rule one mapping
/// follows across its records and one input across the inputs of the manifest. A later declaration
/// of a name already taken is not resolved at all, and one that gives its name nothing leaves
/// nothing of it behind, not even the metadata it recorded.
pub(crate) struct AttributeClaims<'m> {
    /// The mappings carrying the entity, in carried order.
    mappings: &'m [Arc<Mapping>],
    /// Which declaration each attribute taken so far is taken from.
    owners: AttributeOwners,
}

impl<'m> AttributeClaims<'m> {
    /// The claims of an entity carried by `mappings`, or `None` when one mapping carries it and every
    /// declaration is the only one of its name.
    pub(crate) fn of(mappings: &'m [Arc<Mapping>]) -> Option<AttributeClaims<'m>> {
        (mappings.len() > 1).then(|| AttributeClaims {
            mappings,
            owners: AttributeOwners::new(mappings),
        })
    }

    /// Whether a mapping carried before the one declaring `name` at `at` already took `name`.
    pub(crate) fn taken_before(&self, at: CarriedDeclaration, name: &NameBuf) -> bool {
        self.mappings
            .iter()
            .take(at.mapping)
            .enumerate()
            .any(|(mapping, carried)| self.owned_by(mapping, carried, name))
    }

    /// Whether any carried mapping took `name`.
    pub(crate) fn taken(&self, name: &NameBuf) -> bool {
        self.mappings.iter().enumerate().any(|(mapping, carried)| self.owned_by(mapping, carried, name))
    }

    /// Settles the declaration of `name` at `at` by its `claim`: a declaration that takes the
    /// attribute owns it, and one that yields drops the metadata it recorded for it.
    pub(crate) fn settle(&mut self, at: CarriedDeclaration, name: &NameBuf, claim: Claim, metadata: &mut EntityMetadata) {
        match claim {
            Claim::Takes => self.owners.claim(at),
            Claim::Yields => {
                metadata.swap_remove(name);
            }
        }
    }

    /// The record of which declaration each attribute was taken from.
    pub(crate) fn into_owners(self) -> AttributeOwners {
        self.owners
    }

    /// Whether `carried`, the mapping carried at `mapping`, declares `name` and took it.
    fn owned_by(&self, mapping: usize, carried: &Mapping, name: &NameBuf) -> bool {
        carried
            .attributes()
            .get_index_of(name)
            .is_some_and(|declaration| self.owners.owns(CarriedDeclaration { mapping, declaration }))
    }
}

#[cfg(test)]
mod tests {
    use crate::{dropped_attributes::DroppedAttributes, entity_extractor::EntityExtractor, extractor::Extractor};
    use cassiopeia_batch_sink::batch_sink::BatchSink;
    use cassiopeia_expander::compiler::ExpanderCompiler;
    use cassiopeia_ir::{
        assembled_entity::{AssembledEntity, AssembledFragment, AssembledFragments},
        attribute_content::AttributeContent,
        attribute_owners::CarriedDeclaration,
        entity::Entity,
        entity_relationships::EntityRelationships,
        metadata::MetadataStorage,
        relationships::Relationships,
        resolved_attribute::ResolvedAttribute,
    };
    use cassiopeia_mapping::{
        mapping::Mapping,
        template::{run_variables::RunVariables, runner::TemplateRunner},
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    /// A station labelled by its record, with a reading in degrees Celsius.
    const CELSIUS: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ code }}" },
        attributes: {
            label: { source: "{{ label }}" },
            temperature: { source: "{{ t }}", transformation: "float", properties: { unitCode: { source: "CEL" } } },
        },
    }"#;

    /// A station labelled by its record, with a reading that declares no qualifier, and a status.
    const PLAIN: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ code }}" },
        attributes: {
            label: { source: "{{ label }}" },
            temperature: { source: "{{ t }}", transformation: "float" },
            status: { source: "{{ status }}" },
        },
    }"#;

    /// A station whose label must read as an integer, so text that does not is refused and recorded.
    const NUMBERED: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ code }}" },
        attributes: { label: { source: "{{ label }}", transformation: "integer" } },
    }"#;

    /// A station operated by an operator, qualified by the operator's role.
    const OPERATED_IN_A_ROLE: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ code }}" },
        attributes: {
            operatedBy: { type: "Relationship", source: "{{ operator }}", target: { entity: "Operator" }, properties: { role: { source: "primary" } } },
        },
    }"#;

    /// A station operated by an operator, qualified by a note.
    const OPERATED_WITH_A_NOTE: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ code }}" },
        attributes: {
            operatedBy: { type: "Relationship", source: "{{ operator }}", target: { entity: "Operator" }, properties: { note: { source: "secondary" } } },
        },
    }"#;

    /// A station operated by an operator, qualified by a date its record must hold for the qualifier,
    /// and so the relationship, to resolve.
    const OPERATED_SINCE: &str = r#"{
        version: "v4",
        dataModel: "Station",
        identity: { entityName: "{{ code }}" },
        attributes: {
            operatedBy: { type: "Relationship", source: "{{ operator }}", target: { entity: "Operator" }, properties: { since: { source: "{{ since | upper }}" } } },
        },
    }"#;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    fn urn(value: &str) -> Urn {
        value.parse::<Urn>().unwrap()
    }

    /// Relationship objects that name the operator `O1` under `operatedBy`.
    fn operated_by_o1() -> EntityRelationships {
        let mut top_level = Relationships::default();
        top_level.insert(name("operatedBy"), vec![urn("urn:ngsi-ld:Operator:O1")]);
        EntityRelationships::new(top_level, None, None)
    }

    /// Extracts the station `S1`, carried by one fragment per mapping document and record of
    /// `fragments`, in their order, with the relationship objects `relationships`.
    fn extract(fragments: Vec<(&str, JsonValue)>, relationships: EntityRelationships, dropped: &DroppedAttributes) -> Entity {
        let mut runner = TemplateRunner::new();
        let fragments: AssembledFragments = fragments
            .into_iter()
            .map(|(document, record)| {
                let mut mapping = Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap();
                ExpanderCompiler::compile(&mut mapping, Path::new("test.json5"), &mut runner).unwrap();
                AssembledFragment::new(record, None, Arc::new(mapping), RunVariables::none())
            })
            .collect();
        let assembled = AssembledEntity::new(urn("urn:ngsi-ld:Station:S1"), None, None, relationships, fragments);
        EntityExtractor::new(runner.resolver()).extract(assembled, dropped).unwrap().into_parts().0
    }

    /// The value the single-instance attribute `attribute` of `entity` resolved to, as JSON.
    fn value(entity: &Entity, attribute: &str) -> Option<JsonValue> {
        match entity.values().as_ref()?.get(&name(attribute))? {
            ResolvedAttribute::Single(AttributeContent::Value(value)) => Some(serde_json::to_value(value).unwrap()),
            other @ (ResolvedAttribute::Single(AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_))
            | ResolvedAttribute::Instances(_)) => panic!("expected one value of `{attribute}`, got {other:?}"),
        }
    }

    /// The names of the shared sub-attributes recorded for `attribute` of `entity`, none when it has
    /// no metadata.
    fn qualifier_names(entity: &Entity, attribute: &str) -> Vec<String> {
        entity
            .metadata()
            .as_ref()
            .and_then(|metadata| metadata.get(&name(attribute)))
            .and_then(MetadataStorage::as_shared)
            .map(|shared| shared.keys().map(ToString::to_string).collect())
            .unwrap_or_default()
    }

    /// Whether the declaration at index `declaration` of the mapping carried at `mapping` owns its
    /// attribute in `entity`.
    fn owns(entity: &Entity, mapping: usize, declaration: usize) -> bool {
        entity
            .attribute_owners()
            .as_ref()
            .is_some_and(|owners| owners.owns(CarriedDeclaration { mapping, declaration }))
    }

    #[test]
    fn an_entity_one_mapping_carries_records_no_attribute_owners() {
        let entity = extract(
            vec![(CELSIUS, json!({"code": "S1", "label": "North", "t": 21.5}))],
            EntityRelationships::default(),
            &DroppedAttributes::new(),
        );

        assert_eq!(entity.attribute_owners(), &None);
        assert_eq!(value(&entity, "label"), Some(json!("North")));
    }

    #[test]
    fn an_entity_two_mappings_carry_records_the_declaration_each_attribute_is_taken_from() {
        let entity = extract(
            vec![
                (CELSIUS, json!({"code": "S1", "label": "North"})),
                (PLAIN, json!({"code": "S1", "label": "South", "t": 70.7, "status": "ok"})),
            ],
            EntityRelationships::default(),
            &DroppedAttributes::new(),
        );

        let owned: Vec<bool> = [(0, 0), (0, 1), (1, 0), (1, 1), (1, 2)]
            .into_iter()
            .map(|(mapping, declaration)| owns(&entity, mapping, declaration))
            .collect();
        assert_eq!(owned, [true, false, false, true, true]);
        assert_eq!(value(&entity, "label"), Some(json!("North")));
        assert_eq!(value(&entity, "temperature"), Some(json!(70.7)));
    }

    #[test]
    fn a_later_declaration_of_a_name_an_earlier_fragment_took_is_never_resolved() {
        let dropped = DroppedAttributes::new();

        let entity = extract(
            vec![
                (PLAIN, json!({"code": "S1", "label": "North"})),
                (NUMBERED, json!({"code": "S1", "label": "not a number"})),
            ],
            EntityRelationships::default(),
            &dropped,
        );

        assert_eq!(value(&entity, "label"), Some(json!("North")));
        assert!(dropped.unusable_values.is_empty());
    }

    #[test]
    fn a_name_the_first_fragment_gives_no_value_is_taken_from_the_next_without_the_first_fragment_s_metadata() {
        let entity = extract(
            vec![(CELSIUS, json!({"code": "S1"})), (PLAIN, json!({"code": "S1", "t": 70.7}))],
            EntityRelationships::default(),
            &DroppedAttributes::new(),
        );

        assert_eq!(value(&entity, "temperature"), Some(json!(70.7)));
        assert_eq!(qualifier_names(&entity, "temperature"), Vec::<String>::new());
        assert!(owns(&entity, 1, 1));
        assert!(!owns(&entity, 0, 1));
    }

    #[test]
    fn a_relationship_two_mappings_declare_is_taken_from_the_first_with_its_own_properties() {
        let entity = extract(
            vec![
                (OPERATED_IN_A_ROLE, json!({"code": "S1", "operator": "O1"})),
                (OPERATED_WITH_A_NOTE, json!({"code": "S1", "operator": "O1"})),
            ],
            operated_by_o1(),
            &DroppedAttributes::new(),
        );

        assert!(owns(&entity, 0, 0));
        assert!(!owns(&entity, 1, 0));
        assert_eq!(qualifier_names(&entity, "operatedBy"), ["role"]);
        assert_eq!(entity.relationships().get(&name("operatedBy")), Some(&vec![urn("urn:ngsi-ld:Operator:O1")]));
    }

    #[test]
    fn a_relationship_whose_properties_fail_in_the_first_mapping_is_taken_by_the_next_and_keeps_its_objects() {
        let dropped = DroppedAttributes::new();

        let entity = extract(
            vec![
                (OPERATED_SINCE, json!({"code": "S1", "operator": "O1"})),
                (OPERATED_WITH_A_NOTE, json!({"code": "S1", "operator": "O1"})),
            ],
            operated_by_o1(),
            &dropped,
        );

        assert!(owns(&entity, 1, 0));
        assert_eq!(qualifier_names(&entity, "operatedBy"), ["note"]);
        assert_eq!(entity.relationships().get(&name("operatedBy")), Some(&vec![urn("urn:ngsi-ld:Operator:O1")]));
        assert_eq!(
            BatchSink::from(dropped.templates)
                .into_entries()
                .map(|(attribute, _)| attribute)
                .collect::<Vec<_>>(),
            [name("operatedBy")]
        );
    }

    #[test]
    fn a_relationship_whose_properties_fail_in_the_only_mapping_declaring_it_is_dropped_with_its_objects() {
        let entity = extract(
            vec![
                (OPERATED_SINCE, json!({"code": "S1", "operator": "O1"})),
                (PLAIN, json!({"code": "S1", "label": "South"})),
            ],
            operated_by_o1(),
            &DroppedAttributes::new(),
        );

        assert!(!entity.relationships().contains_key(&name("operatedBy")));
        assert_eq!(value(&entity, "label"), Some(json!("South")));
    }
}
