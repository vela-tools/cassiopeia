use crate::urn::{
    builder::UrnBuilder,
    error::{Result, UrnError},
    id_segment::IdSegment,
    non_text_shape::NonTextShape,
};
use cassiopeia_mapping::{
    attribute::{Attribute, instance::AttributeInstance},
    mapping::Mapping,
    scope::{CompiledScope, CompiledScopeTemplate},
    template::{
        CompiledTemplate,
        TemplatePart,
        resolver::{TemplateResolver, collect_identifiers},
        template_input::TemplateInput,
    },
};
use cassiopeia_ngsi_ld::entity::{
    name::NameBuf,
    scope::{NgsiLdScope, ScopeBuf},
};
use serde_json::Value;
use std::slice;
use urn_rs::Urn;

/// Mints NGSI-LD URNs and scopes for expanded entities.
///
/// The generator is stateless apart from the resolver, which shares its templating engine through an
/// `Arc`, so every pipeline worker can hold its own clone without contending on a lock, and a record
/// mints the same URN whichever worker expands it and in whichever order.
#[derive(Clone)]
pub struct UrnGenerator {
    /// The shared resolver used to evaluate identity, scope, and relationship templates.
    resolver: TemplateResolver,
}

impl UrnGenerator {
    /// Creates a generator over a shared template resolver.
    #[must_use]
    pub const fn new(resolver: TemplateResolver) -> UrnGenerator {
        UrnGenerator { resolver }
    }

    /// Generates the entity URN for one record under `mapping`.
    ///
    /// The identity is used exactly as it resolves for the record: a constant identity names one
    /// entity, so every record it maps contributes to that one entity. Records that resolve to the
    /// same URN are merged by the resolver, which reports the merge.
    ///
    /// # Errors
    ///
    /// Returns [`UrnError::GeneratedIdEmpty`] when the identity resolves to null, to empty text, to
    /// a composite with any field absent, or to text with no URN-safe character, and another
    /// [`UrnError`] when the identity template is missing or the built URN is malformed.
    pub fn generate_id(&self, mapping: &Mapping, input: TemplateInput<'_>) -> Result<Urn> {
        let entity_type = mapping.data_model().entity_type();
        let template = mapping.identity().compiled_entity_name().as_ref().ok_or(UrnError::RelationshipMissingSource)?;
        // The identity resolves through the same definition of "the identifier a source produces" as
        // a relationship object does, so an identity over an absent field names no entity.
        let raw_id = self.resolve_identifier(slice::from_ref(template), entity_type, input)?;

        Self::generate_target_id(entity_type, &raw_id)
    }

    /// Generates the scope, or scopes, for an entity from its mapping's `scope` declaration.
    ///
    /// A scope template that resolves to null or an empty string contributes no scope, and one that
    /// resolves to a list contributes one scope per item. A declaration that yields nothing at all
    /// resolves to `None` rather than an empty list, and one whose templates or items resolve to the
    /// same scope yields it once.
    ///
    /// # Errors
    ///
    /// Returns [`UrnError`] when a scope template fails to resolve, reads or resolves to a value that
    /// is not text (see [`UrnError::ScopeFieldNotText`], [`UrnError::ScopeValueNotText`] and
    /// [`UrnError::ScopeItemNotText`]), or its resolved text cannot be the scope of a written entity:
    /// text that is not a legal NGSI-LD scope, or the NGSI-LD Null.
    pub fn generate_scope(&self, mapping: &Mapping, input: TemplateInput<'_>) -> Result<Option<NgsiLdScope>> {
        let templates = match mapping.compiled_scope() {
            Some(CompiledScope::Single(template)) => slice::from_ref(template),
            Some(CompiledScope::Multiple(templates)) => templates.as_slice(),
            None => return Ok(None),
        };

        let mut scopes = Vec::with_capacity(templates.len());
        for template in templates {
            self.collect_scopes(template, input, &mut scopes)?;
        }

        Ok(NgsiLdScope::from_scopes(scopes))
    }

    /// Generates the URN of the entity a relationship attribute points at.
    ///
    /// The target identifier comes from the attribute's compiled source, falling back to its raw
    /// source for a relationship declared as a bare literal.
    ///
    /// # Errors
    ///
    /// Returns [`UrnError::GeneratedIdEmpty`] when the source names no target, because a field it
    /// reads is absent or its text has no URN-safe character, and another [`UrnError`] when the
    /// relationship has no usable source, no target, or the built URN is malformed.
    pub fn generate_child_id(&self, attribute: &Attribute, input: TemplateInput<'_>) -> Result<Urn> {
        let target = attribute.target().as_ref().ok_or(UrnError::NoRelationshipTarget)?;
        let raw_id = match attribute.compiled_source() {
            Some(templates) => self.resolve_identifier(templates, target.entity().entity_type(), input)?,
            // No compiled source: fall back to the raw literal, cloned because the attribute is only
            // borrowed here and the identifier must be owned to build the URN.
            None => match attribute.source() {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Number(number)) => number.to_string(),
                Some(Value::Bool(boolean)) => boolean.to_string(),
                Some(Value::Null | Value::Array(_) | Value::Object(_)) | None => {
                    return Err(UrnError::RelationshipMissingSource);
                }
            },
        };

        Self::generate_target_id(target.entity().entity_type(), &raw_id)
    }

    /// Mints the target URN for one instance of a multi-attribute Relationship from that instance's
    /// own `source` template.
    ///
    /// Each instance of a multi-attribute Relationship (ETSI GS CIM 009 v1.9.1 clause 4.5.5) declares
    /// its own object-id source; every instance shares the attribute-level `target`. An instance
    /// whose source resolves to no identifier yields [`UrnError::GeneratedIdEmpty`], so the caller
    /// records no object for it; the instance is then absent from the objects keyed by instance.
    ///
    /// # Errors
    ///
    /// Returns [`UrnError`] when the relationship declares no target, the instance has no source, or
    /// the built URN is malformed.
    pub fn generate_instance_child_id(&self, instance: &AttributeInstance, attribute: &Attribute, input: TemplateInput<'_>) -> Result<Urn> {
        let target = attribute.target().as_ref().ok_or(UrnError::NoRelationshipTarget)?;
        let raw_id = match instance.compiled_source() {
            Some(templates) => self.resolve_identifier(templates, target.entity().entity_type(), input)?,
            None => return Err(UrnError::RelationshipMissingSource),
        };

        Self::generate_target_id(target.entity().entity_type(), &raw_id)
    }

    /// Mints the target URNs for one instance of a multi-attribute `ListRelationship`, one per token
    /// of that instance's `objectList` source that names a target.
    ///
    /// The instance source is tokenized exactly as a plain list relationship's is
    /// ([`resolve_tokens`](TemplateResolver::resolve_tokens)) and minted through the same
    /// `mint_targets`, so a token that names no target is dropped here too. An
    /// instance left with no token yields an empty vector; the caller records each object under its
    /// instance, so an empty instance is simply absent (ETSI GS CIM 009 v1.9.1 clause 4.5.5, EXAMPLE
    /// 19).
    ///
    /// # Errors
    ///
    /// Returns [`UrnError`] when the relationship declares no target, a source template fails to
    /// resolve, or a built URN is malformed.
    pub fn generate_instance_child_ids(&self, instance: &AttributeInstance, attribute: &Attribute, input: TemplateInput<'_>) -> Result<Vec<Urn>> {
        let target = attribute.target().as_ref().ok_or(UrnError::NoRelationshipTarget)?;
        let tokens = match instance.compiled_source() {
            Some(templates) => self.resolver.resolve_tokens(templates, input)?,
            None => Vec::new(),
        };

        Self::mint_targets(target.entity().entity_type(), &tokens)
    }

    /// Generates the URNs of every entity a list-relationship attribute points at.
    ///
    /// Unlike a single relationship, the source is read as a collection of identifiers: an array
    /// contributes one identifier per element, and a string contributes one per whitespace- or
    /// comma-separated token, so a single field holding several ids (a route's space-separated
    /// equipment codes, say) fans out into one relationship object each. A token that names no target
    /// is dropped (see `mint_targets`).
    ///
    /// # Errors
    /// Returns [`UrnError`] when the relationship declares no target, a source template fails to
    /// resolve, or a built URN is malformed.
    pub fn generate_child_ids(&self, attribute: &Attribute, input: TemplateInput<'_>) -> Result<Vec<Urn>> {
        let target = attribute.target().as_ref().ok_or(UrnError::NoRelationshipTarget)?;

        Self::mint_targets(target.entity().entity_type(), &self.source_identifiers(attribute, input)?)
    }

    /// Mints one target URN per identifier, dropping each identifier that names no target.
    ///
    /// An identifier that cleans to nothing (only characters with no URN-safe form, such as `•` or
    /// `?`) names no entity, exactly as an absent single-relationship foreign key does: that one object
    /// is left out and the rest of the list, and the entity carrying it, are still produced. Every
    /// other failure is a real fault and propagates.
    fn mint_targets(entity_type: &NameBuf, identifiers: &[String]) -> Result<Vec<Urn>> {
        identifiers
            .iter()
            .filter_map(|identifier| match Self::generate_target_id(entity_type, identifier) {
                Ok(urn) => Some(Ok(urn)),
                Err(UrnError::GeneratedIdEmpty { .. }) => None,
                Err(other) => Some(Err(other)),
            })
            .collect()
    }

    /// Reads the identifiers an attribute's source contributes, without minting any URN.
    ///
    /// The tokenisation matches [`generate_child_ids`](Self::generate_child_ids): an array yields one
    /// identifier per element and a string one per whitespace- or comma-separated token, empty tokens
    /// dropped. A token that is present but names no target is returned; the caller decides how to
    /// skip it. A caller that fans an attribute out into one entity per identifier (a list
    /// relationship that also materialises each target as a synthetic entity) shares this splitting
    /// so the link and the emitted entity are keyed on the very same tokens.
    ///
    /// # Errors
    /// Returns [`UrnError`] when a source template fails to resolve.
    pub fn source_identifiers(&self, attribute: &Attribute, input: TemplateInput<'_>) -> Result<Vec<String>> {
        if let Some(templates) = attribute.compiled_source() {
            Ok(self.resolver.resolve_tokens(templates, input)?)
        } else {
            // No compiled source: tokenize the raw literal source, cloned because the attribute is
            // only borrowed here and the tokens must be owned.
            let mut identifiers = Vec::new();
            if let Some(source) = attribute.source() {
                collect_identifiers(source.clone(), &mut identifiers);
            }
            Ok(identifiers)
        }
    }

    /// Builds a target URN from an entity type and an already-resolved raw identifier.
    ///
    /// # Errors
    ///
    /// Returns [`UrnError::GeneratedIdEmpty`] when `raw_id` cleans to nothing, and
    /// [`UrnError::BuildUrn`] when the built URN is malformed.
    pub fn generate_target_id(target_entity_type: &NameBuf, raw_id: &str) -> Result<Urn> {
        UrnBuilder::build(target_entity_type.as_str(), &Self::segment(target_entity_type, raw_id)?)
    }

    /// Resolves the raw identifier a source's templates join into, an absent one (see
    /// [`resolve_joined`](TemplateResolver::resolve_joined)) being an empty identifier for
    /// `entity_type`.
    fn resolve_identifier(&self, templates: &[CompiledTemplate], entity_type: &NameBuf, input: TemplateInput<'_>) -> Result<String> {
        self.resolver
            .resolve_joined(templates, input)?
            .ok_or_else(|| Self::empty_identifier(entity_type))
    }

    /// Cleans a raw identifier into the URN's identifier segment, failing when nothing survives.
    fn segment(entity_type: &NameBuf, raw_id: &str) -> Result<IdSegment> {
        IdSegment::clean(raw_id).ok_or_else(|| Self::empty_identifier(entity_type))
    }

    /// The error for an identifier that names no `entity_type` entity.
    fn empty_identifier(entity_type: &NameBuf) -> UrnError {
        UrnError::GeneratedIdEmpty {
            // The error owns the type name after the borrowed mapping or target is dropped.
            target_entity_type: entity_type.clone(),
        }
    }

    /// Resolves one scope template and appends the scopes it contributes to `scopes`, treating an
    /// absent field anywhere in the template, or an empty result, as "no scope".
    ///
    /// A scope path is only meaningful whole: `/{{ country }}/{{ city }}` over a record with no city
    /// must contribute no scope rather than `/Slovenia/null`, which is a legal but wrong scope. For
    /// the same reason a value that is not text is never written as its JSON rendering: a scope is a
    /// string of levels (ETSI GS CIM 009 v1.9.1 clause 4.18), and `true` or `/Sensors/true` would be
    /// legal but meaningless. Only a number printed into a longer scope, as in `/Floor{{ floor }}`,
    /// is part of a level the source meant. A list resolved whole is the one value that is not text
    /// and still names scopes: an entity may belong to several (clause 4.18), so each item is a scope
    /// of its own.
    fn collect_scopes(&self, template: &CompiledScopeTemplate, input: TemplateInput<'_>, scopes: &mut Vec<ScopeBuf>) -> Result<()> {
        match &template.compiled {
            CompiledTemplate::Composite(parts) => Self::refuse_non_text_parts(template, parts, input)?,
            CompiledTemplate::Static(_) | CompiledTemplate::Simple(_) | CompiledTemplate::Expression(_) | CompiledTemplate::Complex(_) => {}
        }

        match self.resolver.resolve_complete(&template.compiled, input)? {
            Some(Value::String(text)) => Self::push_scope(text, scopes),
            Some(Value::Array(items)) => items
                .into_iter()
                .zip(1..)
                .try_for_each(|(item, position)| Self::collect_item_scope(template, item, position, scopes)),
            Some(other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_))) => {
                NonTextShape::of(&other).map_or(Ok(()), |shape| Err(Self::non_text_scope(template, shape)))
            }
            None => Ok(()),
        }
    }

    /// Appends the scope one item of a list-valued scope template names, at `position` counted from
    /// 1. A null or empty item names no scope, as a null or empty field does.
    fn collect_item_scope(template: &CompiledScopeTemplate, item: Value, position: usize, scopes: &mut Vec<ScopeBuf>) -> Result<()> {
        match item {
            Value::String(text) => Self::push_scope(text, scopes),
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => {
                NonTextShape::of(&other).map_or(Ok(()), |shape| {
                    Err(UrnError::ScopeItemNotText {
                        // The error outlives the record and the mapping the template is borrowed from.
                        template: template.source.clone(),
                        position,
                        shape,
                    })
                })
            }
        }
    }

    /// Validates resolved scope text and appends it to `scopes`; empty text names no scope.
    fn push_scope(text: String, scopes: &mut Vec<ScopeBuf>) -> Result<()> {
        if !text.is_empty() {
            scopes.push(Self::parse_scope(text)?);
        }
        Ok(())
    }

    /// Refuses a concatenated scope template whose field holds a boolean, an array or an object,
    /// none of which prints as a level the source meant.
    fn refuse_non_text_parts(template: &CompiledScopeTemplate, parts: &[TemplatePart], input: TemplateInput<'_>) -> Result<()> {
        parts.iter().try_for_each(|part| match part {
            TemplatePart::Static(_) => Ok(()),
            TemplatePart::Dynamic(field) => match field.lookup(input).and_then(NonTextShape::of) {
                None | Some(NonTextShape::Number) => Ok(()),
                Some(shape @ (NonTextShape::Boolean | NonTextShape::Array | NonTextShape::Object)) => Err(UrnError::ScopeFieldNotText {
                    // The error outlives the record and the mapping the template is borrowed from.
                    template: template.source.clone(),
                    field: field.clone(),
                    shape,
                }),
            },
        })
    }

    /// The error for a scope template that resolved to a value of `shape` rather than text, naming
    /// the field when the template reads one field whole.
    fn non_text_scope(template: &CompiledScopeTemplate, shape: NonTextShape) -> UrnError {
        // The error outlives the record and the mapping the template is borrowed from.
        let source = template.source.clone();
        match &template.compiled {
            CompiledTemplate::Simple(field) => UrnError::ScopeFieldNotText {
                template: source,
                field: field.clone(),
                shape,
            },
            CompiledTemplate::Static(_) | CompiledTemplate::Composite(_) | CompiledTemplate::Expression(_) | CompiledTemplate::Complex(_) => {
                UrnError::ScopeValueNotText { template: source, shape }
            }
        }
    }

    /// Validates resolved scope text into the scope of the entity being written.
    fn parse_scope(text: String) -> Result<ScopeBuf> {
        match ScopeBuf::for_written_entity(&text) {
            Ok(scope) => Ok(scope),
            Err(source) => Err(UrnError::InvalidScope {
                rejected: text.into_boxed_str(),
                source,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compiler::ExpanderCompiler,
        urn::{error::UrnError, generator::UrnGenerator, non_text_shape::NonTextShape},
    };
    use cassiopeia_mapping::{
        mapping::Mapping,
        template::{run_variables::RunVariables, runner::TemplateRunner, template_input::TemplateInput},
    };
    use cassiopeia_ngsi_ld::entity::error::NgsiLdError;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::path::Path;

    /// A generator and the compiled mapping `document` declares.
    fn generator(document: &str) -> (UrnGenerator, Mapping) {
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, Path::new("test.json5"), &mut runner).unwrap();

        (UrnGenerator::new(runner.resolver()), mapping)
    }

    /// A generator and an `AircraftType` mapping whose identity is `identity` and whose `codeIATA`
    /// attribute reads a different value from every record.
    fn aircraft_type_generator(identity: &str) -> (UrnGenerator, Mapping) {
        generator(
            &r#"{
            version: "v4",
            dataModel: "AircraftType",
            identity: { entityName: "IDENTITY" },
            attributes: { codeIATA: { source: "{{ code }}" } },
        }"#
            .replace("IDENTITY", identity),
        )
    }

    /// Generates the scope of a `Thing` whose mapping declares `scope` for one record, as its wire
    /// form.
    fn generate_thing_scope(scope: &str, data: &Value) -> Result<Option<Value>, UrnError> {
        let (generator, mapping) = generator(&format!(
            r#"{{ version: "v4", dataModel: "Thing", identity: {{ entityName: "{{{{ id }}}}" }}, scope: {scope}, attributes: {{}} }}"#
        ));

        generator
            .generate_scope(&mapping, TemplateInput::new(data, &RunVariables::none()))
            .map(|scope| scope.map(|scope| serde_json::to_value(scope).unwrap()))
    }

    fn is_null_scope_rejection(result: &Result<Option<Value>, UrnError>) -> bool {
        matches!(
            result,
            Err(UrnError::InvalidScope { rejected, source: NgsiLdError::NullScope }) if &**rejected == "urn:ngsi-ld:null"
        )
    }

    #[test]
    fn a_scope_resolving_to_the_ngsi_ld_null_is_rejected() {
        let result = generate_thing_scope(r#""{{ s }}""#, &json!({"id": "B", "s": "urn:ngsi-ld:null"}));

        assert!(is_null_scope_rejection(&result), "{result:?}");
    }

    #[test]
    fn one_scope_of_a_list_resolving_to_the_ngsi_ld_null_rejects_the_whole_scope() {
        let result = generate_thing_scope(r#"["/Madrid", "{{ s }}"]"#, &json!({"id": "B", "s": "urn:ngsi-ld:null"}));

        assert!(is_null_scope_rejection(&result), "{result:?}");
    }

    #[test]
    fn a_scope_resolving_to_a_legal_scope_is_generated() {
        let result = generate_thing_scope(r#""{{ s }}""#, &json!({"id": "A", "s": "/Madrid"}));

        assert_eq!(result.unwrap(), Some(json!("/Madrid")));
    }

    #[test]
    fn a_scope_resolving_to_a_malformed_scope_is_rejected_as_illegal() {
        let result = generate_thing_scope(r#""{{ s }}""#, &json!({"id": "A", "s": "/Madrid Centro"}));

        assert!(
            matches!(
                &result,
                Err(UrnError::InvalidScope {
                    source: NgsiLdError::InvalidScope { .. },
                    ..
                })
            ),
            "{result:?}"
        );
    }

    /// Whether `result` refuses the scope template `template` for reading `field` as `shape`.
    fn is_field_not_text(result: &Result<Option<Value>, UrnError>, template: &str, field: &str, shape: NonTextShape) -> bool {
        matches!(
            result,
            Err(UrnError::ScopeFieldNotText { template: refused, field: read, shape: held })
                if refused.as_str() == template && read.as_str() == field && *held == shape
        )
    }

    #[test]
    fn a_scope_reading_a_boolean_field_whole_is_refused_naming_the_field_and_its_type() {
        let result = generate_thing_scope(r#""{{ indoor }}""#, &json!({"id": "A", "indoor": true}));

        assert!(is_field_not_text(&result, "{{ indoor }}", "indoor", NonTextShape::Boolean), "{result:?}");
        assert_eq!(
            result.unwrap_err().to_string(),
            "The scope template `{{ indoor }}` reads `indoor`, which holds a boolean, not a scope"
        );
    }

    #[test]
    fn a_scope_printing_a_boolean_field_into_a_level_is_refused() {
        let result = generate_thing_scope(r#""/Sensors/{{ indoor }}""#, &json!({"id": "B", "indoor": false}));

        assert!(
            is_field_not_text(&result, "/Sensors/{{ indoor }}", "indoor", NonTextShape::Boolean),
            "{result:?}"
        );
    }

    #[test]
    fn a_scope_list_with_a_template_reading_a_boolean_field_is_refused() {
        let result = generate_thing_scope(r#"["/Madrid", "/Sensors/{{ indoor }}"]"#, &json!({"id": "A", "indoor": true}));

        assert!(
            is_field_not_text(&result, "/Sensors/{{ indoor }}", "indoor", NonTextShape::Boolean),
            "{result:?}"
        );
    }

    #[test]
    fn a_scope_reading_a_number_or_an_object_whole_is_refused_naming_its_type() {
        for (value, shape) in [(json!(3), NonTextShape::Number), (json!({"city": "Madrid"}), NonTextShape::Object)] {
            let result = generate_thing_scope(r#""{{ zone }}""#, &json!({"id": "A", "zone": value}));

            assert!(is_field_not_text(&result, "{{ zone }}", "zone", shape), "{value}: {result:?}");
        }
    }

    #[test]
    fn a_scope_printing_an_array_or_an_object_into_a_level_is_refused() {
        for (value, shape) in [(json!(["Madrid"]), NonTextShape::Array), (json!({"city": "Madrid"}), NonTextShape::Object)] {
            let result = generate_thing_scope(r#""/Zone/{{ zone }}""#, &json!({"id": "A", "zone": value}));

            assert!(is_field_not_text(&result, "/Zone/{{ zone }}", "zone", shape), "{value}: {result:?}");
        }
    }

    #[test]
    fn a_scope_printing_a_number_into_a_level_is_generated() {
        let result = generate_thing_scope(r#""/Floor{{ floor }}""#, &json!({"id": "A", "floor": 3}));

        assert_eq!(result.unwrap(), Some(json!("/Floor3")));
    }

    #[test]
    fn a_scope_expression_resolving_to_a_number_is_refused_naming_its_type() {
        let result = generate_thing_scope(r#""{{ floor + 1 }}""#, &json!({"id": "A", "floor": 3}));

        assert!(
            matches!(&result, Err(UrnError::ScopeValueNotText { template, shape: NonTextShape::Number }) if template.as_str() == "{{ floor + 1 }}"),
            "{result:?}"
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "The scope template `{{ floor + 1 }}` resolves to a number, not a scope"
        );
    }

    #[test]
    fn a_scope_over_a_null_or_missing_field_still_gives_no_scope() {
        for record in [json!({"id": "A", "indoor": null}), json!({"id": "A"})] {
            for scope in [r#""{{ indoor }}""#, r#""/Sensors/{{ indoor }}""#] {
                let result = generate_thing_scope(scope, &record);

                assert_eq!(result.unwrap(), None, "{scope} over {record}");
            }
        }
    }

    #[test]
    fn a_scope_reading_a_list_field_whole_gives_one_scope_per_item() {
        let result = generate_thing_scope(r#""{{ areas }}""#, &json!({"id": "A", "areas": ["/Ljubljana", "/Ljubljana/Center"]}));

        assert_eq!(result.unwrap(), Some(json!(["/Ljubljana", "/Ljubljana/Center"])));
    }

    #[test]
    fn a_scope_reading_a_one_item_list_field_gives_a_single_scope() {
        let result = generate_thing_scope(r#""{{ areas }}""#, &json!({"id": "A", "areas": ["/Ljubljana"]}));

        assert_eq!(result.unwrap(), Some(json!("/Ljubljana")));
    }

    #[test]
    fn a_list_field_in_a_scope_list_joins_the_other_scopes_without_duplicates() {
        let result = generate_thing_scope(
            r#"["/Ljubljana", "{{ areas }}"]"#,
            &json!({"id": "A", "areas": ["/Ljubljana/Center", "/Ljubljana"]}),
        );

        assert_eq!(result.unwrap(), Some(json!(["/Ljubljana", "/Ljubljana/Center"])));
    }

    #[test]
    fn a_scope_reading_a_list_with_no_scope_items_gives_no_scope() {
        for areas in [json!([]), json!([null, ""])] {
            let result = generate_thing_scope(r#""{{ areas }}""#, &json!({"id": "A", "areas": areas}));

            assert_eq!(result.unwrap(), None, "{areas}");
        }
    }

    #[test]
    fn a_scope_expression_resolving_to_a_list_gives_one_scope_per_item() {
        let result = generate_thing_scope(r#""{{ areas | reverse }}""#, &json!({"id": "A", "areas": ["/Ljubljana", "/Maribor"]}));

        assert_eq!(result.unwrap(), Some(json!(["/Ljubljana", "/Maribor"])));
    }

    #[test]
    fn a_list_item_that_is_not_text_is_refused_naming_its_position_and_type() {
        for (item, shape) in [
            (json!(3), NonTextShape::Number),
            (json!(true), NonTextShape::Boolean),
            (json!(["/Maribor"]), NonTextShape::Array),
            (json!({"city": "Maribor"}), NonTextShape::Object),
        ] {
            let result = generate_thing_scope(r#""{{ areas }}""#, &json!({"id": "A", "areas": ["/Ljubljana", item]}));

            assert!(
                matches!(
                    &result,
                    Err(UrnError::ScopeItemNotText { template, position: 2, shape: held }) if template.as_str() == "{{ areas }}" && *held == shape
                ),
                "{item}: {result:?}"
            );
        }
    }

    #[test]
    fn a_list_item_that_is_not_text_is_named_by_itself_in_the_message() {
        let result = generate_thing_scope(r#""{{ areas }}""#, &json!({"id": "A", "areas": ["/Ljubljana", 3]}));

        assert_eq!(
            result.unwrap_err().to_string(),
            "The scope template `{{ areas }}` gives a list whose item 2 holds a number, not a scope"
        );
    }

    #[test]
    fn a_list_item_that_is_not_a_legal_scope_is_rejected_quoting_the_item_alone() {
        let result = generate_thing_scope(r#""{{ areas }}""#, &json!({"id": "A", "areas": ["/Ljubljana", "/Novo mesto"]}));

        assert!(
            matches!(
                &result,
                Err(UrnError::InvalidScope { rejected, source: NgsiLdError::InvalidScope { .. } }) if &**rejected == "/Novo mesto"
            ),
            "{result:?}"
        );
    }

    #[test]
    fn a_list_item_resolving_to_the_ngsi_ld_null_is_rejected() {
        let result = generate_thing_scope(r#""{{ areas }}""#, &json!({"id": "A", "areas": ["/Ljubljana", "urn:ngsi-ld:null"]}));

        assert!(is_null_scope_rejection(&result), "{result:?}");
    }

    /// Mints the identity of an `AircraftType` named by `identity` for one record.
    fn generate_aircraft_type_id(identity: &str, data: &Value) -> Result<String, UrnError> {
        let (generator, mapping) = aircraft_type_generator(identity);

        generator
            .generate_id(&mapping, TemplateInput::new(data, &RunVariables::none()))
            .map(|urn| urn.to_string())
    }

    fn is_generated_id_empty(result: &Result<String, UrnError>) -> bool {
        matches!(result, Err(UrnError::GeneratedIdEmpty { target_entity_type }) if target_entity_type.as_str() == "AircraftType")
    }

    #[test]
    fn an_identity_resolving_to_null_is_empty_rather_than_the_text_null() {
        let result = generate_aircraft_type_id("{{ code }}", &json!({"code": null}));

        assert!(is_generated_id_empty(&result), "{result:?}");
    }

    #[test]
    fn an_identity_over_a_missing_field_is_empty_rather_than_the_text_null() {
        let result = generate_aircraft_type_id("{{ code }}", &json!({}));

        assert!(is_generated_id_empty(&result), "{result:?}");
    }

    #[test]
    fn a_composite_identity_over_a_missing_null_or_empty_field_is_empty_rather_than_the_text_null() {
        for record in [json!({}), json!({"code": null}), json!({"code": ""})] {
            let result = generate_aircraft_type_id("Type-{{ code }}", &record);

            assert!(is_generated_id_empty(&result), "{record}: {result:?}");
        }
    }

    #[test]
    fn a_composite_identity_with_its_field_present_mints_its_urn() {
        let result = generate_aircraft_type_id("Type-{{ code }}", &json!({"code": 320}));

        assert_eq!(result.unwrap(), "urn:ngsi-ld:AircraftType:Type-320");
    }

    #[test]
    fn an_identity_resolving_to_empty_text_is_empty() {
        let result = generate_aircraft_type_id("{{ code }}", &json!({"code": ""}));

        assert!(is_generated_id_empty(&result), "{result:?}");
    }

    #[test]
    fn a_tera_identity_rendering_empty_text_is_empty() {
        let result = generate_aircraft_type_id("{% if code %}{{ code }}{% endif %}", &json!({"code": null}));

        assert!(is_generated_id_empty(&result), "{result:?}");
    }

    #[test]
    fn an_identity_cleaning_to_nothing_is_empty() {
        let result = generate_aircraft_type_id("{{ code }}", &json!({"code": "!?"}));

        assert!(is_generated_id_empty(&result), "{result:?}");
    }

    #[test]
    fn a_literal_identity_cleaning_to_nothing_is_empty() {
        let result = generate_aircraft_type_id("!?", &json!({"code": "E7W"}));

        assert!(is_generated_id_empty(&result), "{result:?}");
    }

    #[test]
    fn a_literal_identity_in_a_one_record_feed_mints_the_identity_as_written() {
        let result = generate_aircraft_type_id("AircraftType", &json!({"code": "E7W"}));

        assert_eq!(result.unwrap(), "urn:ngsi-ld:AircraftType:AircraftType");
    }

    #[test]
    fn a_literal_identity_mints_the_same_urn_for_every_record_whatever_its_attributes_read() {
        let (generator, mapping) = aircraft_type_generator("AircraftType");

        let urns: Vec<String> = ["E7W", "320", "77W"]
            .into_iter()
            .map(|code| {
                generator
                    .generate_id(&mapping, TemplateInput::new(&json!({ "code": code }), &RunVariables::none()))
                    .unwrap()
                    .to_string()
            })
            .collect();

        assert_eq!(urns, ["urn:ngsi-ld:AircraftType:AircraftType"; 3]);
    }

    #[test]
    fn a_present_identity_mints_its_urn() {
        let result = generate_aircraft_type_id("{{ code }}", &json!({"code": "E7W"}));

        assert_eq!(result.unwrap(), "urn:ngsi-ld:AircraftType:E7W");
    }

    #[test]
    fn a_numeric_identity_mints_its_digits() {
        let result = generate_aircraft_type_id("{{ code }}", &json!({"code": 320}));

        assert_eq!(result.unwrap(), "urn:ngsi-ld:AircraftType:320");
    }
}
