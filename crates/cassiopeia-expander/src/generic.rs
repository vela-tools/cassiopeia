use crate::{
    error::ExpanderError,
    expander::Expander,
    router::MappingRouter,
    urn::{error::UrnError, generator::UrnGenerator},
};
use cassiopeia_common::parallelism::Parallelism;
use cassiopeia_ir::{
    fragment::Fragment,
    instance_index::InstanceIndex,
    mapped::Mapped,
    parent_context::{ParentContext, ParentContextType},
    record::Record,
    relationship_key::RelationshipKey,
    relationship_path::RelationshipPath,
};
use cassiopeia_mapping::{attribute::Attribute, mapping::Mapping, template::resolver::TemplateResolver};
use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};
use rayon::prelude::*;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use urn_rs::Urn;

/// Expands records into fragments, routing each to the mapping its source collection selects.
///
/// A `GenericExpander` is immutable after construction and holds its mappings behind `Arc`s via a
/// [`MappingRouter`], so it can be shared across the rayon workers that expand a batch. It expects
/// every mapping's templates to have already been compiled (see
/// [`ExpanderCompiler`](crate::compiler::ExpanderCompiler)) and the resolver to have been taken from
/// the runner afterwards. The single shared [`UrnGenerator`] serves every mapping, since the mapping
/// is passed per call rather than bound into the generator.
pub struct GenericExpander {
    router: MappingRouter,
    urn_generator: UrnGenerator,
    parallelism: Parallelism,
    /// The run-level variables injected into every record this lane expands, under the reserved
    /// top-level `vars` key, so a mapping reads them as `{{ vars.<name> }}`. Empty when the run
    /// declares none, in which case no key is inserted.
    vars: Map<String, Value>,
}

impl GenericExpander {
    /// Creates an expander for an already-compiled set of mappings and the resolver that backs them.
    ///
    /// `vars` is this lane's effective run-level variables (manifest-global overlaid by CLI `--var`
    /// and this input's own `vars`), injected into each record under the reserved `vars` key.
    #[must_use]
    pub fn new(router: MappingRouter, resolver: TemplateResolver, vars: Map<String, Value>) -> GenericExpander {
        GenericExpander {
            router,
            urn_generator: UrnGenerator::new(resolver),
            parallelism: Parallelism::Parallel,
            vars,
        }
    }

    /// Inserts the lane's run-level variables into a record object under the reserved `vars` key,
    /// leaving the object untouched when the lane declares no variables.
    fn inject_vars(&self, object: &mut Map<String, Value>) {
        if !self.vars.is_empty() {
            object.insert("vars".to_string(), Value::Object(self.vars.clone()));
        }
    }

    /// Sets whether batch expansion runs across threads.
    #[must_use]
    pub const fn with_parallelism(mut self, parallelism: Parallelism) -> GenericExpander {
        self.parallelism = parallelism;
        self
    }

    /// Expands one record's data into a main fragment plus any synthetic-entity fragments, under the
    /// mapping the router selected for it.
    fn expand_value(&self, mapping: &Arc<Mapping>, data: Value) -> Result<Vec<Mapped<Fragment>>, ExpanderError> {
        let mut fragments = Vec::new();
        let mut child_contexts = Vec::new();

        let main_urn = self.urn_generator.generate_id(mapping, &data)?;
        let main_scope = self.urn_generator.generate_scope(mapping, &data)?;

        for (attribute_name, attribute) in mapping.attributes() {
            if let Some(synthetic) = attribute.synthetic_entity() {
                if attribute.target().is_some() && matches!(attribute.kind(), NgsiLdAttributeKind::ListRelationship) {
                    self.expand_synthetic_targets(attribute_name, attribute, synthetic, &data, &mut fragments, &mut child_contexts)?;
                } else if let Some(fragment) = self.expand_synthetic_entity(attribute_name, attribute, synthetic, &main_urn, &data)? {
                    fragments.push(fragment);
                }
            } else if matches!(attribute.kind(), NgsiLdAttributeKind::ListRelationship) && attribute.target().is_some() && attribute.instances().is_some() {
                // A list relationship carrying instances (ETSI GS CIM 009 v1.9.1 clause 4.5.5, EXAMPLE
                // 19) fans each instance's own source into child edges, each recorded under the
                // instance that minted it, so the instance boundaries reach the transformer intact
                // however many tokens an instance dropped.
                for (index, instance) in attribute.instances().iter().flatten().enumerate() {
                    for child_urn in self.urn_generator.generate_instance_child_ids(instance, attribute, &data)? {
                        child_contexts.push(ParentContext::new(
                            ParentContextType::Child(child_urn),
                            Self::instance_key(attribute_name, index),
                        ));
                    }
                }
            } else if attribute.target().is_some() && matches!(attribute.kind(), NgsiLdAttributeKind::ListRelationship) {
                // A list relationship fans its source out into one object per identifier, so a field
                // holding several ids links to each of them.
                for child_urn in self.urn_generator.generate_child_ids(attribute, &data)? {
                    child_contexts.push(ParentContext::new(ParentContextType::Child(child_urn), Self::top_level_key(attribute_name)));
                }
            } else if Self::is_relationship(attribute) && attribute.target().is_some() {
                if let Some(instances) = attribute.instances() {
                    // A multi-attribute Relationship (clause 4.5.5) mints one object per instance,
                    // recorded under that instance; an instance whose foreign key is absent
                    // contributes no link and so has no object under its index.
                    for (index, instance) in instances.iter().enumerate() {
                        match self.urn_generator.generate_instance_child_id(instance, attribute, &data) {
                            Ok(child_urn) => {
                                child_contexts.push(ParentContext::new(
                                    ParentContextType::Child(child_urn),
                                    Self::instance_key(attribute_name, index),
                                ));
                            }
                            Err(UrnError::GeneratedIdEmpty { .. }) => {}
                            Err(other) => return Err(other.into()),
                        }
                    }
                } else {
                    match self.urn_generator.generate_child_id(attribute, &data) {
                        Ok(child_urn) => {
                            child_contexts.push(ParentContext::new(ParentContextType::Child(child_urn), Self::top_level_key(attribute_name)));
                        }
                        // A relationship whose foreign key is absent in this record contributes no
                        // link; the entity is still produced without it, rather than dropped over one
                        // missing optional value.
                        Err(UrnError::GeneratedIdEmpty { .. }) => {}
                        Err(other) => return Err(other.into()),
                    }
                }
            }
        }

        // A mapping that declares a nested relationship mints its objects too, under a multi-segment
        // path (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3). The gate keeps this recursion off
        // the hot path entirely for the common mapping that declares none.
        if mapping.has_nested_relationships() {
            for (attribute_name, attribute) in mapping.attributes() {
                let path = RelationshipPath::flat(attribute_name.clone());
                self.mint_nested_relationships(attribute, &path, &data, &mut child_contexts)?;
            }
        }

        let parent_context = if child_contexts.is_empty() { None } else { Some(child_contexts) };
        let main_fragment = Fragment::new(data, main_urn, main_scope, parent_context);
        fragments.push(Mapped::new(main_fragment, Arc::clone(mapping)));

        Ok(fragments)
    }

    /// Materialises one target per identifier of a list relationship that also declares a synthetic
    /// entity, linking the main entity to each.
    ///
    /// The field is split into its tokens, and each token both links the main entity to a target and
    /// is emitted as that target's own entity. Each token is bound as a single-column positional
    /// record, so the synthetic mapping reads it with the same `this[0]` accessor a headerless source
    /// uses. Minting the id once and reusing it for the link keeps the relationship and the emitted
    /// entity in step, so a token whose synthetic identity names no target contributes neither: the
    /// rest of the list, and the main entity, are still produced.
    fn expand_synthetic_targets(
        &self,
        attribute_name: &NameBuf,
        attribute: &Attribute,
        synthetic: &Mapping,
        data: &Value,
        fragments: &mut Vec<Mapped<Fragment>>,
        child_contexts: &mut Vec<ParentContext>,
    ) -> Result<(), ExpanderError> {
        for token in self.urn_generator.source_identifiers(attribute, data)? {
            let mut token_data = json!({ "0": token });
            if let Value::Object(object) = &mut token_data {
                self.inject_vars(object);
            }
            let synthetic_urn = match self.urn_generator.generate_id(synthetic, &token_data) {
                Ok(urn) => urn,
                Err(UrnError::GeneratedIdEmpty { .. }) => continue,
                Err(other) => return Err(other.into()),
            };
            let synthetic_scope = self.urn_generator.generate_scope(synthetic, &token_data)?;

            child_contexts.push(ParentContext::new(
                ParentContextType::Child(synthetic_urn.clone()),
                Self::top_level_key(attribute_name),
            ));
            let fragment = Fragment::new(token_data, synthetic_urn, synthetic_scope, None);
            // The fragment owns its mapping behind an `Arc`, and the synthetic mapping is only
            // borrowed from the parent attribute here.
            fragments.push(Mapped::new(fragment, Arc::new(synthetic.clone())));
        }

        Ok(())
    }

    /// Expands the one synthetic entity a non-list attribute materialises, linked back to the main
    /// entity through a parent context, or nothing when this record names no target.
    ///
    /// The link the main entity carries is minted from the synthetic identity, so it always points at
    /// the entity emitted here. A record whose synthetic identity, or whose relationship source,
    /// resolves to no identifier has no target to emit or link: the attribute contributes nothing and
    /// the main entity is still produced, exactly as a plain Relationship with an absent foreign key
    /// and a `ListRelationship` with no tokens behave.
    fn expand_synthetic_entity(
        &self,
        attribute_name: &NameBuf,
        attribute: &Attribute,
        synthetic: &Mapping,
        main_urn: &Urn,
        data: &Value,
    ) -> Result<Option<Mapped<Fragment>>, ExpanderError> {
        if attribute.target().is_some() && attribute.source().is_some() {
            // The minted object is discarded: this only asks whether the relationship source names a
            // target, under the very definition the plain Relationship path uses.
            match self.urn_generator.generate_child_id(attribute, data) {
                Ok(_) => {}
                Err(UrnError::GeneratedIdEmpty { .. }) => return Ok(None),
                Err(other) => return Err(other.into()),
            }
        }

        let synthetic_urn = match self.urn_generator.generate_id(synthetic, data) {
            Ok(urn) => urn,
            Err(UrnError::GeneratedIdEmpty { .. }) => return Ok(None),
            Err(other) => return Err(other.into()),
        };
        let synthetic_scope = self.urn_generator.generate_scope(synthetic, data)?;

        // The parent context owns its URN and path, while the main entity keeps its own for its fragment.
        let parent = ParentContext::new(ParentContextType::Parent(main_urn.clone()), Self::top_level_key(attribute_name));
        // Each synthetic fragment carries its own copy of the source data; the main fragment consumes
        // the record once every attribute has been expanded.
        let fragment = Fragment::new(data.clone(), synthetic_urn, synthetic_scope, Some(vec![parent]));
        // The fragment owns its mapping behind an `Arc`, and the synthetic mapping is only borrowed
        // from the parent attribute here.
        Ok(Some(Mapped::new(fragment, Arc::new(synthetic.clone()))))
    }

    /// Mints the objects of every relationship declared as a sub-attribute of `attribute`, recording
    /// each under its full [`RelationshipPath`] from the entity.
    ///
    /// The same minting primitives serve top-level and nested relationships, so a nested object is
    /// shaped identically to a top-level one; only the path differs (multi-segment here). A single
    /// nested Relationship whose foreign key is absent in this record mints nothing, dropped in
    /// lockstep with the extractor's per-path lookup, exactly as a top-level relationship is. The walk
    /// descends every sub-attribute, since a Property sub-attribute may itself carry a nested
    /// relationship, and a nested relationship a further one.
    fn mint_nested_relationships(
        &self,
        attribute: &Attribute,
        path: &RelationshipPath,
        data: &Value,
        child_contexts: &mut Vec<ParentContext>,
    ) -> Result<(), ExpanderError> {
        let Some(properties) = attribute.properties() else {
            return Ok(());
        };

        for (name, property) in properties {
            let child_path = path.push(name.clone());
            if property.target().is_some() {
                match property.kind() {
                    NgsiLdAttributeKind::Relationship => match self.urn_generator.generate_child_id(property, data) {
                        Ok(child_urn) => child_contexts.push(ParentContext::new(
                            ParentContextType::Child(child_urn),
                            RelationshipKey::Path(child_path.clone()),
                        )),
                        // A nested relationship whose foreign key is absent contributes no link, exactly
                        // as an absent top-level relationship does.
                        Err(UrnError::GeneratedIdEmpty { .. }) => {}
                        Err(other) => return Err(other.into()),
                    },
                    NgsiLdAttributeKind::ListRelationship => {
                        for child_urn in self.urn_generator.generate_child_ids(property, data)? {
                            child_contexts.push(ParentContext::new(
                                ParentContextType::Child(child_urn),
                                RelationshipKey::Path(child_path.clone()),
                            ));
                        }
                    }
                    NgsiLdAttributeKind::Property
                    | NgsiLdAttributeKind::GeoProperty
                    | NgsiLdAttributeKind::LanguageProperty
                    | NgsiLdAttributeKind::VocabProperty
                    | NgsiLdAttributeKind::ListProperty
                    | NgsiLdAttributeKind::JsonProperty => {}
                }
            }
            self.mint_nested_relationships(property, &child_path, data, child_contexts)?;
        }

        Ok(())
    }

    /// The key a top-level relationship records its objects under: its one-segment path.
    fn top_level_key(attribute_name: &NameBuf) -> RelationshipKey {
        RelationshipKey::Path(RelationshipPath::flat(attribute_name.clone()))
    }

    /// The key one instance of a multi-attribute relationship records its objects under: the
    /// attribute and the instance's declaration index (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
    fn instance_key(attribute_name: &NameBuf, index: usize) -> RelationshipKey {
        RelationshipKey::Instance {
            attribute: attribute_name.clone(),
            index: InstanceIndex::from(index),
        }
    }

    /// Whether an attribute produces a relationship (single or list).
    fn is_relationship(attribute: &Attribute) -> bool {
        matches!(attribute.kind(), NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship)
    }
}

impl Expander for GenericExpander {
    fn expand(&self, record: Record) -> Result<Vec<Mapped<Fragment>>, ExpanderError> {
        let mapping = self.router.select(record.collection().as_ref())?;
        let mut data = record.into_data();
        self.inject_vars(&mut data);
        self.expand_value(mapping, Value::Object(data))
    }

    fn expand_batch(&self, records: Vec<Record>) -> Vec<Result<Vec<Mapped<Fragment>>, ExpanderError>> {
        match self.parallelism {
            Parallelism::Parallel => records.into_par_iter().map(|record| self.expand(record)).collect(),
            Parallelism::Sequential => records.into_iter().map(|record| self.expand(record)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compiler::ExpanderCompiler,
        error::ExpanderError,
        expander::Expander,
        generic::GenericExpander,
        router::{CollectionRoutes, MappingRouter},
        urn::error::UrnError,
    };
    use cassiopeia_common::collection::CollectionName;
    use cassiopeia_ir::{fragment::Fragment, mapped::Mapped, parent_context::ParentContextType, record::Record};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use serde_json::{Map, Value, json};
    use std::{path::Path, sync::Arc};

    fn compiled(document: &str, runner: &mut TemplateRunner) -> Arc<Mapping> {
        let mut mapping = Mapping::from_json5(document, Path::new("test.json5"), runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, Path::new("test.json5"), runner).unwrap();
        Arc::new(mapping)
    }

    fn expander(document: &str) -> GenericExpander {
        expander_with_vars(document, Map::new())
    }

    fn expander_with_vars(document: &str, vars: Map<String, Value>) -> GenericExpander {
        let mut runner = TemplateRunner::new();
        let mapping = compiled(document, &mut runner);
        let resolver = runner.resolver();

        GenericExpander::new(MappingRouter::Single(mapping), resolver, vars)
    }

    fn record(data: Value) -> Record {
        record_in(None, data)
    }

    fn record_in(collection: Option<CollectionName>, data: Value) -> Record {
        let Value::Object(map) = data else {
            panic!("test record must be a JSON object");
        };
        Record::new(collection, map)
    }

    fn urn(fragment: &Mapped<Fragment>) -> String {
        fragment.inner().target_urn().to_string()
    }

    #[test]
    fn a_dynamic_identity_produces_a_single_fragment_with_the_resolved_urn() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "AirQualityObserved",
                identity: { entityName: "Station-{{ id }}" },
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "temperature": 21.5}))).unwrap();

        assert_eq!(fragments.len(), 1);
        assert_eq!(urn(&fragments[0]), "urn:ngsi-ld:AirQualityObserved:Station-1");
    }

    #[test]
    fn a_run_variable_resolves_in_the_identity() {
        let mut vars = Map::new();
        vars.insert("region".to_string(), json!("Ljubljana"));
        let expander = expander_with_vars(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "{{ vars.region }}-{{ id }}" },
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
            vars,
        );

        let fragments = expander.expand(record(json!({"id": 1, "temperature": 21.5}))).unwrap();

        assert_eq!(urn(&fragments[0]), "urn:ngsi-ld:Sensor:Ljubljana-1");
    }

    #[test]
    fn a_run_variable_is_absent_when_the_lane_declares_none() {
        // With no vars injected there is no `vars` key, so `{{ vars.region }}` reads as a missing
        // field, stringifying to `null` in a composite, exactly as any other absent field would.
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "S{{ vars.region }}-{{ id }}" },
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "temperature": 21.5}))).unwrap();

        assert_eq!(urn(&fragments[0]), "urn:ngsi-ld:Sensor:Snull-1");
    }

    #[test]
    fn a_run_variable_reaches_a_per_token_synthetic_entity() {
        let mut vars = Map::new();
        vars.insert("suffix".to_string(), json!("X"));
        let expander = expander_with_vars(
            r#"{
                version: "v4",
                dataModel: "Mountain",
                identity: { entityName: "M-{{ id }}" },
                attributes: {
                    hasCountry: {
                        type: "ListRelationship",
                        source: "{{ countries }}",
                        target: { entity: "Country" },
                        syntheticEntity: {
                            dataModel: "Country",
                            identity: { entityName: "{{ this[0] }}-{{ vars.suffix }}" },
                            attributes: { name: { source: "{{ this[0] }}" } },
                        },
                    },
                },
            }"#,
            vars,
        );

        let fragments = expander.expand(record(json!({"id": 1, "countries": "Nepal, China"}))).unwrap();
        let urns: Vec<String> = fragments.iter().map(urn).collect();

        assert!(urns.contains(&"urn:ngsi-ld:Country:Nepal-X".to_string()));
        assert!(urns.contains(&"urn:ngsi-ld:Country:China-X".to_string()));
    }

    #[test]
    fn a_mapping_scope_becomes_the_fragment_scope() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "AirQualityObserved",
                identity: { entityName: "S-{{ id }}" },
                scope: "/Ljubljana",
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "temperature": 21.5}))).unwrap();
        let scope = fragments[0].inner().scope().as_ref().unwrap();

        assert_eq!(serde_json::to_value(scope).unwrap(), json!("/Ljubljana"));
    }

    #[test]
    fn a_list_of_mapping_scopes_resolves_per_record_into_the_fragment_scope() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "AirQualityObserved",
                identity: { entityName: "S-{{ id }}" },
                scope: ["/{{ country }}", "/{{ country }}/{{ city }}"],
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
        );

        let fragments = expander
            .expand(record(json!({"id": 1, "country": "Slovenia", "city": "Ljubljana", "temperature": 21.5})))
            .unwrap();
        let scope = fragments[0].inner().scope().as_ref().unwrap();

        assert_eq!(serde_json::to_value(scope).unwrap(), json!(["/Slovenia", "/Slovenia/Ljubljana"]));
    }

    #[test]
    fn scope_templates_resolving_to_the_same_scope_yield_it_once() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "AirQualityObserved",
                identity: { entityName: "S-{{ id }}" },
                scope: ["/{{ city }}", "/{{ town }}"],
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
        );

        let fragments = expander
            .expand(record(json!({"id": 1, "city": "Ljubljana", "town": "Ljubljana", "temperature": 21.5})))
            .unwrap();
        let scope = fragments[0].inner().scope().as_ref().unwrap();

        assert_eq!(serde_json::to_value(scope).unwrap(), json!("/Ljubljana"));
    }

    #[test]
    fn a_scope_template_with_a_missing_field_contributes_no_scope() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "AirQualityObserved",
                identity: { entityName: "S-{{ id }}" },
                scope: ["/{{ country }}", "/{{ country }}/{{ city }}"],
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "country": "Slovenia", "temperature": 21.5}))).unwrap();
        let scope = fragments[0].inner().scope().as_ref().unwrap();

        assert_eq!(serde_json::to_value(scope).unwrap(), json!("/Slovenia"));
    }

    #[test]
    fn a_single_scope_template_over_a_missing_field_leaves_the_fragment_unscoped() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "AirQualityObserved",
                identity: { entityName: "S-{{ id }}" },
                scope: "/{{ city }}",
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "temperature": 21.5}))).unwrap();

        assert!(fragments[0].inner().scope().is_none());
    }

    #[test]
    fn a_mapping_without_a_scope_produces_an_unscoped_fragment() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "AirQualityObserved",
                identity: { entityName: "S-{{ id }}" },
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "temperature": 21.5}))).unwrap();

        assert!(fragments[0].inner().scope().is_none());
    }

    #[test]
    fn a_synthetic_entity_takes_its_own_scope_not_its_parents() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "Building",
                identity: { entityName: "B-{{ id }}" },
                scope: "/Buildings",
                attributes: {
                    owner: {
                        type: "Relationship",
                        source: "{{ owner_id }}",
                        target: { entity: "Organization" },
                        syntheticEntity: {
                            dataModel: "Organization",
                            identity: { entityName: "{{ owner_id }}" },
                            scope: "/Organizations/{{ region }}",
                            attributes: { name: { source: "{{ owner_name }}" } },
                        },
                    },
                },
            }"#,
        );

        let fragments = expander
            .expand(record(json!({"id": 1, "owner_id": "acme", "owner_name": "ACME", "region": "Coast"})))
            .unwrap();
        let scope_of = |target: &str| {
            let fragment = fragments.iter().find(|fragment| urn(fragment) == target).unwrap();
            serde_json::to_value(fragment.inner().scope().as_ref().unwrap()).unwrap()
        };

        assert_eq!(scope_of("urn:ngsi-ld:Building:B-1"), json!("/Buildings"));
        assert_eq!(scope_of("urn:ngsi-ld:Organization:acme"), json!("/Organizations/Coast"));
    }

    #[test]
    fn a_relationship_attribute_attaches_a_child_context_to_the_main_fragment() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "AirQualityObserved",
                identity: { entityName: "Station-{{ id }}" },
                attributes: {
                    refRoad: { source: "{{ road_id }}", type: "Relationship", target: { entity: "Road" } },
                },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "road_id": 99}))).unwrap();

        assert_eq!(fragments.len(), 1);
        let contexts = fragments[0].inner().parent_context().as_ref().unwrap();
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].key().to_string(), "refRoad");
        match contexts[0].urn() {
            ParentContextType::Child(child) => assert_eq!(child.to_string(), "urn:ngsi-ld:Road:99"),
            ParentContextType::Parent(_) => panic!("relationship must produce a child context"),
        }
    }

    #[test]
    fn a_synthetic_entity_produces_a_second_fragment_linked_to_the_main_one() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "AirQualityObserved",
                identity: { entityName: "Station-{{ id }}" },
                attributes: {
                    device: {
                        syntheticEntity: {
                            version: "v4",
                            dataModel: "Device",
                            identity: { entityName: "Dev-{{ id }}" },
                            attributes: { name: { source: "{{ name }}" } },
                        },
                    },
                },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "name": "sensor"}))).unwrap();

        assert_eq!(fragments.len(), 2);
        let urns: Vec<String> = fragments.iter().map(urn).collect();
        assert!(urns.contains(&"urn:ngsi-ld:Device:Dev-1".to_string()));
        assert!(urns.contains(&"urn:ngsi-ld:AirQualityObserved:Station-1".to_string()));

        let device = fragments.iter().find(|fragment| urn(fragment).contains("Device")).unwrap();
        let parent = device.inner().parent_context().as_ref().unwrap();
        match parent[0].urn() {
            ParentContextType::Parent(main) => assert_eq!(main.to_string(), "urn:ngsi-ld:AirQualityObserved:Station-1"),
            ParentContextType::Child(_) => panic!("synthetic entity must link back to its parent"),
        }
    }

    #[test]
    fn a_list_relationship_with_a_synthetic_entity_materialises_one_target_per_token() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "Mountain",
                identity: { entityName: "M-{{ id }}" },
                attributes: {
                    hasCountry: {
                        type: "ListRelationship",
                        source: "{{ countries }}",
                        target: { entity: "Country" },
                        syntheticEntity: {
                            dataModel: "Country",
                            identity: { entityName: "{{ this[0] }}" },
                            attributes: { name: { source: "{{ this[0] }}" } },
                        },
                    },
                },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "countries": "Nepal, China"}))).unwrap();

        // The main Mountain plus one materialised Country per token.
        assert_eq!(fragments.len(), 3);
        let urns: Vec<String> = fragments.iter().map(urn).collect();
        assert!(urns.contains(&"urn:ngsi-ld:Country:Nepal".to_string()));
        assert!(urns.contains(&"urn:ngsi-ld:Country:China".to_string()));
        assert!(urns.contains(&"urn:ngsi-ld:Mountain:M-1".to_string()));

        // The main entity links to both countries through the one list relationship.
        let mountain = fragments.iter().find(|fragment| urn(fragment).contains("Mountain")).unwrap();
        let contexts = mountain.inner().parent_context().as_ref().unwrap();
        assert_eq!(contexts.len(), 2);
        for context in contexts {
            match context.urn() {
                ParentContextType::Child(child) => assert!(child.to_string().starts_with("urn:ngsi-ld:Country:")),
                ParentContextType::Parent(_) => panic!("list relationship must produce child contexts"),
            }
        }

        // A materialised target is emitted as an entity in its own right, not linked back as a child.
        let country = fragments.iter().find(|fragment| urn(fragment).contains("Country")).unwrap();
        assert!(country.inner().parent_context().is_none());
    }

    /// An `AircraftModel` mapping over a headerless `planes.dat` record whose single
    /// `belongsToAircraftType` Relationship materialises its target as a synthetic `AircraftType`
    /// named by `synthetic_identity`.
    fn aircraft_expander(synthetic_identity: &str) -> GenericExpander {
        let document = r#"{
            version: "v4",
            dataModel: "AircraftModel",
            identity: { entityName: "{{ this[2] }}" },
            attributes: {
                belongsToAircraftType: {
                    source: "{% if this[1] %}{{ this[1] }}{% endif %}",
                    type: "Relationship",
                    target: { entity: "AircraftType" },
                    syntheticEntity: {
                        dataModel: "AircraftType",
                        identity: { entityName: "SYNTHETIC_IDENTITY" },
                        attributes: { codeIATA: { source: "{{ this[1] }}", type: "Property", transformation: "string" } },
                    },
                },
            },
        }"#;
        expander(&document.replace("SYNTHETIC_IDENTITY", synthetic_identity))
    }

    /// Asserts that a record whose aircraft-type code is absent yields only the main `BE58` fragment,
    /// with no synthetic fragment and no relationship context.
    fn assert_only_the_main_aircraft_model(fragments: &[Mapped<Fragment>]) {
        assert_eq!(fragments.len(), 1);
        assert_eq!(urn(&fragments[0]), "urn:ngsi-ld:AircraftModel:BE58");
        assert!(fragments[0].inner().parent_context().is_none());
    }

    #[test]
    fn a_synthetic_relationship_whose_identity_reads_a_null_field_emits_no_target_and_no_link() {
        let expander = aircraft_expander("{{ this[1] }}");

        let fragments = expander.expand(record(json!({"0": "Beechcraft Baron", "1": null, "2": "BE58"}))).unwrap();

        assert_only_the_main_aircraft_model(&fragments);
    }

    #[test]
    fn a_synthetic_relationship_whose_guarded_identity_renders_empty_emits_no_target_and_no_link() {
        let expander = aircraft_expander("{% if this[1] %}{{ this[1] }}{% endif %}");

        let fragments = expander.expand(record(json!({"0": "Beechcraft Baron", "1": null, "2": "BE58"}))).unwrap();

        assert_only_the_main_aircraft_model(&fragments);
    }

    #[test]
    fn a_synthetic_relationship_whose_identity_reads_empty_text_emits_no_target_and_no_link() {
        let expander = aircraft_expander("{{ this[1] }}");

        let fragments = expander.expand(record(json!({"0": "Beechcraft Baron", "1": "", "2": "BE58"}))).unwrap();

        assert_only_the_main_aircraft_model(&fragments);
    }

    #[test]
    fn a_synthetic_relationship_whose_source_is_empty_emits_no_target_even_when_its_identity_resolves() {
        // The identity's static prefix keeps it non-empty, so only the relationship source, which the
        // guard renders empty, can tell that this record names no aircraft type.
        let expander = aircraft_expander("Type-{{ this[2] }}");

        let fragments = expander.expand(record(json!({"0": "Beechcraft Baron", "1": null, "2": "BE58"}))).unwrap();

        assert_only_the_main_aircraft_model(&fragments);
    }

    #[test]
    fn a_synthetic_relationship_with_a_present_code_emits_its_target_linked_to_the_main_entity() {
        let expander = aircraft_expander("{{ this[1] }}");

        let fragments = expander
            .expand(record(json!({"0": "Embraer 175 (long wing)", "1": "E7W", "2": "E75L"})))
            .unwrap();

        assert_eq!(fragments.len(), 2);
        let main = fragments.iter().find(|fragment| urn(fragment) == "urn:ngsi-ld:AircraftModel:E75L").unwrap();
        let aircraft_type = fragments.iter().find(|fragment| urn(fragment) == "urn:ngsi-ld:AircraftType:E7W").unwrap();

        // A single synthetic Relationship records its one edge on the synthetic fragment, pointing back
        // at the main entity, rather than as a child context on the main fragment.
        assert!(main.inner().parent_context().is_none());
        let contexts = aircraft_type.inner().parent_context().as_ref().unwrap();
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].key().to_string(), "belongsToAircraftType");
        match contexts[0].urn() {
            ParentContextType::Parent(parent) => assert_eq!(parent.to_string(), "urn:ngsi-ld:AircraftModel:E75L"),
            ParentContextType::Child(_) => panic!("a synthetic entity must link back to its parent"),
        }
    }

    fn child_urns(fragment: &Mapped<Fragment>) -> Vec<String> {
        fragment
            .inner()
            .parent_context()
            .as_ref()
            .expect("parent context")
            .iter()
            .map(|context| match context.urn() {
                ParentContextType::Child(child) => child.to_string(),
                ParentContextType::Parent(_) => panic!("expected a child context"),
            })
            .collect()
    }

    #[test]
    fn a_relationship_with_instances_mints_one_child_per_instance() {
        let expander = expander(
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

        let fragments = expander.expand(record(json!({"id": 1, "dep": 535, "arr": 340}))).unwrap();

        assert_eq!(fragments.len(), 1);
        // Each object is recorded under the instance that minted it.
        assert_eq!(
            edges(&fragments[0]),
            [
                ("servesAirport#0".to_string(), "urn:ngsi-ld:Airport:535".to_string()),
                ("servesAirport#1".to_string(), "urn:ngsi-ld:Airport:340".to_string()),
            ]
        );
    }

    #[test]
    fn a_relationship_instance_with_an_empty_source_mints_nothing_and_the_others_keep_their_index() {
        let expander = expander(
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

        // The middle instance's foreign key `b` is absent in this record, so it mints no object.
        let fragments = expander.expand(record(json!({"id": 1, "a": 1, "c": 3}))).unwrap();

        assert_eq!(
            edges(&fragments[0]),
            [
                ("servesAirport#0".to_string(), "urn:ngsi-ld:Airport:1".to_string()),
                ("servesAirport#2".to_string(), "urn:ngsi-ld:Airport:3".to_string()),
            ]
        );
    }

    #[test]
    fn a_list_relationship_with_instances_records_each_token_under_its_own_instance() {
        let expander = expander(
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

        let fragments = expander.expand(record(json!({"id": 1, "a": "1 2", "b": "3"}))).unwrap();

        assert_eq!(fragments.len(), 1);
        assert_eq!(
            edges(&fragments[0]),
            [
                ("servesAirports#0".to_string(), "urn:ngsi-ld:Airport:1".to_string()),
                ("servesAirports#0".to_string(), "urn:ngsi-ld:Airport:2".to_string()),
                ("servesAirports#1".to_string(), "urn:ngsi-ld:Airport:3".to_string()),
            ]
        );
    }

    /// A `Route` whose `servesAirports` `ListRelationship` declares a departure instance read from `a`
    /// and an arrival instance read from `b` (ETSI GS CIM 009 v1.9.1 clause 4.5.5, EXAMPLE 19).
    fn route_instances_expander(source_a: &str) -> GenericExpander {
        let document = r#"{
            version: "v4",
            dataModel: "Route",
            identity: { entityName: "R-{{ id }}" },
            attributes: {
                servesAirports: {
                    type: "ListRelationship",
                    target: { entity: "Airport" },
                    instances: [
                        { source: "SOURCE_A", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:departure" } } },
                        { source: "{{ b }}", properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:arrival" } } },
                    ],
                },
            },
        }"#;
        expander(&document.replace("SOURCE_A", source_a))
    }

    #[test]
    fn a_list_relationship_instance_drops_an_unmintable_token_and_the_next_instance_keeps_its_index() {
        let expander = route_instances_expander("{{ a }}");

        let fragments = expander.expand(record(json!({"id": 1, "a": "• 2", "b": "3 4"}))).unwrap();

        assert_eq!(
            edges(&fragments[0]),
            [
                ("servesAirports#0".to_string(), "urn:ngsi-ld:Airport:2".to_string()),
                ("servesAirports#1".to_string(), "urn:ngsi-ld:Airport:3".to_string()),
                ("servesAirports#1".to_string(), "urn:ngsi-ld:Airport:4".to_string()),
            ]
        );
    }

    #[test]
    fn a_list_relationship_instance_of_only_unmintable_tokens_mints_nothing_under_its_index() {
        let expander = route_instances_expander("{{ a }}");

        let fragments = expander.expand(record(json!({"id": 1, "a": "• ?", "b": "3"}))).unwrap();

        assert_eq!(edges(&fragments[0]), [("servesAirports#1".to_string(), "urn:ngsi-ld:Airport:3".to_string())]);
    }

    /// An unguarded `split` over a null field is a template failure, not an absent identifier.
    const FAILING_SPLIT: &str = r"{{ a | split(pat=' ') }}";

    #[test]
    fn a_failing_list_relationship_instance_template_still_fails_expansion() {
        let expander = route_instances_expander(FAILING_SPLIT);

        let error = expander.expand(record(json!({"id": 1, "a": null, "b": "3"}))).unwrap_err();

        assert!(matches!(error, ExpanderError::Urn(UrnError::Template(_))), "{error:?}");
    }

    #[test]
    fn a_failing_list_relationship_template_still_fails_expansion() {
        let expander = expander(&format!(
            r#"{{
                version: "v4",
                dataModel: "Route",
                identity: {{ entityName: "R-{{{{ id }}}}" }},
                attributes: {{ usesEquipment: {{ type: "ListRelationship", source: "{FAILING_SPLIT}", target: {{ entity: "Equipment" }} }} }},
            }}"#
        ));

        let error = expander.expand(record(json!({"id": 1, "a": null}))).unwrap_err();

        assert!(matches!(error, ExpanderError::Urn(UrnError::Template(_))), "{error:?}");
    }

    #[test]
    fn a_failing_per_token_synthetic_identity_template_still_fails_expansion() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "Mountain",
                identity: { entityName: "M-{{ id }}" },
                attributes: {
                    hasCountry: {
                        type: "ListRelationship",
                        source: "{{ countries }}",
                        target: { entity: "Country" },
                        syntheticEntity: {
                            dataModel: "Country",
                            identity: { entityName: "{{ missing | split(pat=' ') }}" },
                            attributes: { name: { source: "{{ this[0] }}" } },
                        },
                    },
                },
            }"#,
        );

        let error = expander.expand(record(json!({"id": 1, "countries": "Nepal"}))).unwrap_err();

        assert!(matches!(error, ExpanderError::Urn(UrnError::Template(_))), "{error:?}");
    }

    /// A `Route` mapping whose `usesEquipment` `ListRelationship` reads its targets from `equipment`.
    fn route_equipment_expander() -> GenericExpander {
        expander(
            r#"{
                version: "v4",
                dataModel: "Route",
                identity: { entityName: "R-{{ id }}" },
                attributes: {
                    usesEquipment: { type: "ListRelationship", source: "{{ equipment }}", target: { entity: "Equipment" } },
                },
            }"#,
        )
    }

    #[test]
    fn a_list_relationship_drops_an_unmintable_token_of_delimited_text_and_links_the_rest() {
        let expander = route_equipment_expander();

        // `•` transliterates to `*`, which leaves no URN-safe character, so it names no target.
        let fragments = expander.expand(record(json!({"id": 1, "equipment": "744 • 777"}))).unwrap();

        assert_eq!(fragments.len(), 1);
        assert_eq!(urn(&fragments[0]), "urn:ngsi-ld:Route:R-1");
        assert_eq!(child_urns(&fragments[0]), ["urn:ngsi-ld:Equipment:744", "urn:ngsi-ld:Equipment:777"]);
    }

    #[test]
    fn a_list_relationship_drops_an_unmintable_array_element_and_links_the_rest() {
        let expander = route_equipment_expander();

        let fragments = expander.expand(record(json!({"id": 1, "equipment": ["744", "?", "777"]}))).unwrap();

        assert_eq!(fragments.len(), 1);
        assert_eq!(urn(&fragments[0]), "urn:ngsi-ld:Route:R-1");
        assert_eq!(child_urns(&fragments[0]), ["urn:ngsi-ld:Equipment:744", "urn:ngsi-ld:Equipment:777"]);
    }

    #[test]
    fn a_list_relationship_with_a_synthetic_entity_skips_the_target_and_link_of_an_unmintable_token() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "Mountain",
                identity: { entityName: "M-{{ id }}" },
                attributes: {
                    hasCountry: {
                        type: "ListRelationship",
                        source: "{{ countries }}",
                        target: { entity: "Country" },
                        syntheticEntity: {
                            dataModel: "Country",
                            identity: { entityName: "{{ this[0] }}" },
                            attributes: { name: { source: "{{ this[0] }}" } },
                        },
                    },
                },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "countries": "Nepal, •, China"}))).unwrap();

        let urns: Vec<String> = fragments.iter().map(urn).collect();
        assert_eq!(urns, ["urn:ngsi-ld:Country:Nepal", "urn:ngsi-ld:Country:China", "urn:ngsi-ld:Mountain:M-1"]);
        let mountain = fragments.iter().find(|fragment| urn(fragment) == "urn:ngsi-ld:Mountain:M-1").unwrap();
        assert_eq!(child_urns(mountain), ["urn:ngsi-ld:Country:Nepal", "urn:ngsi-ld:Country:China"]);
    }

    #[test]
    fn a_static_identity_with_varying_attributes_increments_the_urn_per_record() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "Sensor" },
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
        );

        let first = expander.expand(record(json!({"temperature": 20}))).unwrap();
        let second = expander.expand(record(json!({"temperature": 21}))).unwrap();

        assert_eq!(urn(&first[0]), "urn:ngsi-ld:Sensor:Sensor-1");
        assert_eq!(urn(&second[0]), "urn:ngsi-ld:Sensor:Sensor-2");
    }

    fn collections_expander() -> GenericExpander {
        let mut runner = TemplateRunner::new();
        let camera = compiled(
            r#"{ version: "v4", dataModel: "Camera", identity: { entityName: "Cam-{{ id }}" }, attributes: { v: { source: "{{ v }}" } } }"#,
            &mut runner,
        );
        let sensor = compiled(
            r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "Sen-{{ id }}" }, attributes: { v: { source: "{{ v }}" } } }"#,
            &mut runner,
        );
        let resolver = runner.resolver();

        let mut mappings = CollectionRoutes::default();
        mappings.insert(CollectionName::from("Camera"), camera);
        mappings.insert(CollectionName::from("Flowcount"), sensor);
        GenericExpander::new(MappingRouter::Collections(mappings), resolver, Map::new())
    }

    #[test]
    fn a_single_router_applies_its_mapping_whether_or_not_a_collection_is_present() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "Sensor",
                identity: { entityName: "S-{{ id }}" },
                attributes: { temperature: { source: "{{ temperature }}" } },
            }"#,
        );

        let without = expander.expand(record(json!({"id": 1, "temperature": 20}))).unwrap();
        let with = expander
            .expand(record_in(Some(CollectionName::from("Ignored")), json!({"id": 2, "temperature": 21})))
            .unwrap();

        assert_eq!(urn(&without[0]), "urn:ngsi-ld:Sensor:S-1");
        assert_eq!(urn(&with[0]), "urn:ngsi-ld:Sensor:S-2");
    }

    #[test]
    fn a_collections_router_routes_each_label_to_its_own_mapping() {
        let expander = collections_expander();

        let camera = expander
            .expand(record_in(Some(CollectionName::from("Camera")), json!({"id": 7, "v": 1})))
            .unwrap();
        let flow = expander
            .expand(record_in(Some(CollectionName::from("Flowcount")), json!({"id": 9, "v": 2})))
            .unwrap();

        assert_eq!(urn(&camera[0]), "urn:ngsi-ld:Camera:Cam-7");
        assert_eq!(urn(&flow[0]), "urn:ngsi-ld:Sensor:Sen-9");
    }

    #[test]
    fn an_unmatched_label_fails_expansion() {
        let expander = collections_expander();
        let error = expander
            .expand(record_in(Some(CollectionName::from("Nope")), json!({"id": 1, "v": 1})))
            .unwrap_err();
        assert!(matches!(error, ExpanderError::UnmatchedCollection(name) if name == CollectionName::from("Nope")));
    }

    #[test]
    fn a_record_without_a_label_under_collections_fails_expansion() {
        let expander = collections_expander();
        let error = expander.expand(record(json!({"id": 1, "v": 1}))).unwrap_err();
        assert!(matches!(error, ExpanderError::CollectionMissing));
    }

    #[test]
    fn a_temporal_mapping_keeps_the_same_urn_across_records() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "AirQualityObserved",
                identity: { entityName: "Station" },
                attributes: {
                    temperature: {
                        source: "{{ temperature }}",
                        properties: { observedAt: { source: "{{ timestamp }}" } },
                    },
                },
            }"#,
        );

        let first = expander
            .expand(record(json!({"temperature": 20, "timestamp": "2026-04-03T22:00:20Z"})))
            .unwrap();
        let second = expander
            .expand(record(json!({"temperature": 21, "timestamp": "2026-04-03T22:05:20Z"})))
            .unwrap();

        assert_eq!(urn(&first[0]), "urn:ngsi-ld:AirQualityObserved:Station");
        assert_eq!(urn(&second[0]), "urn:ngsi-ld:AirQualityObserved:Station");
    }

    /// Every child edge of a fragment as a `(dotted-path, object-urn)` pair.
    fn edges(fragment: &Mapped<Fragment>) -> Vec<(String, String)> {
        fragment
            .inner()
            .parent_context()
            .as_ref()
            .map(|contexts| {
                contexts
                    .iter()
                    .filter_map(|context| match context.urn() {
                        ParentContextType::Child(child) => Some((context.key().to_string(), child.to_string())),
                        ParentContextType::Parent(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_nested_relationship_sub_attribute_mints_a_child_under_a_two_segment_path() {
        let expander = expander(
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
                            playsCharacter: { type: "Relationship", target: { entity: "Character" }, source: "{{ character }}" },
                            billingOrder: { source: "{{ order }}" },
                        },
                    },
                },
            }"#,
        );

        let fragments = expander
            .expand(record(json!({"id": 1, "actor": 31, "character": "JackSparrow", "order": 0})))
            .unwrap();
        let edges = edges(&fragments[0]);

        // The top-level relationship, plus the nested one under a two-segment path; the nested Property
        // (billingOrder) mints no edge.
        assert!(edges.contains(&("hasLeadActor".to_string(), "urn:ngsi-ld:Person:31".to_string())));
        assert!(edges.contains(&("hasLeadActor.playsCharacter".to_string(), "urn:ngsi-ld:Character:JackSparrow".to_string())));
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn a_nested_list_relationship_sub_attribute_mints_one_child_per_token() {
        let expander = expander(
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

        let fragments = expander.expand(record(json!({"id": 1, "director": 5, "films": "10 20"}))).unwrap();
        let edges = edges(&fragments[0]);

        assert!(edges.contains(&("directedBy.knownFor".to_string(), "urn:ngsi-ld:Movie:10".to_string())));
        assert!(edges.contains(&("directedBy.knownFor".to_string(), "urn:ngsi-ld:Movie:20".to_string())));
    }

    #[test]
    fn a_relationship_nested_two_levels_deep_mints_a_three_segment_path() {
        let expander = expander(
            r#"{
                version: "v4",
                dataModel: "A",
                identity: { entityName: "A-{{ id }}" },
                attributes: {
                    b: {
                        type: "Relationship",
                        target: { entity: "B" },
                        source: "{{ b }}",
                        properties: {
                            c: {
                                type: "Relationship",
                                target: { entity: "C" },
                                source: "{{ c }}",
                                properties: {
                                    d: { type: "Relationship", target: { entity: "D" }, source: "{{ d }}" },
                                },
                            },
                        },
                    },
                },
            }"#,
        );

        let fragments = expander.expand(record(json!({"id": 1, "b": 1, "c": 2, "d": 3}))).unwrap();
        let edges = edges(&fragments[0]);

        assert!(edges.contains(&("b.c.d".to_string(), "urn:ngsi-ld:D:3".to_string())));
    }

    #[test]
    fn a_nested_relationship_with_an_absent_source_mints_nothing() {
        let expander = expander(
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
                            playsCharacter: { type: "Relationship", target: { entity: "Character" }, source: "{{ character }}" },
                        },
                    },
                },
            }"#,
        );

        // The nested relationship's foreign key `character` is absent, so it mints nothing; the
        // top-level relationship still mints its object.
        let fragments = expander.expand(record(json!({"id": 1, "actor": 31}))).unwrap();

        assert_eq!(edges(&fragments[0]), vec![("hasLeadActor".to_string(), "urn:ngsi-ld:Person:31".to_string())]);
    }

    #[test]
    fn a_mapping_without_nested_relationships_reports_false_and_mints_only_the_flat_edge() {
        let mut runner = TemplateRunner::new();
        let mapping = compiled(
            r#"{
                version: "v4",
                dataModel: "Station",
                identity: { entityName: "S-{{ id }}" },
                attributes: { refRoad: { source: "{{ road }}", type: "Relationship", target: { entity: "Road" } } },
            }"#,
            &mut runner,
        );
        assert!(!mapping.has_nested_relationships());
        let resolver = runner.resolver();
        let expander = GenericExpander::new(MappingRouter::Single(mapping), resolver, Map::new());

        let fragments = expander.expand(record(json!({"id": 1, "road": 9}))).unwrap();

        assert_eq!(edges(&fragments[0]), vec![("refRoad".to_string(), "urn:ngsi-ld:Road:9".to_string())]);
    }
}
