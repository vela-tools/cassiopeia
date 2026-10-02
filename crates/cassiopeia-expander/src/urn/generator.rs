use crate::urn::{
    analysis::Analysis,
    builder::UrnBuilder,
    error::{Result, UrnError},
    id_segment::IdSegment,
};
use ahash::RandomState;
use cassiopeia_mapping::{
    attribute::{Attribute, instance::AttributeInstance},
    mapping::Mapping,
    scope::CompiledScope,
    template::{
        CompiledTemplate,
        resolver::{TemplateResolver, collect_identifiers},
    },
};
use cassiopeia_ngsi_ld::entity::{
    name::NameBuf,
    scope::{NgsiLdScope, ScopeBuf},
};
use dashmap::DashMap;
use serde_json::Value;
use std::slice;
use urn_rs::Urn;

/// How a resolved identifier is turned into a unique URN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeduplicationStrategy {
    /// Use the resolved ID exactly, with no suffix.
    Direct,
    /// Append a per-value numeric suffix so repeated static IDs stay distinct.
    Increment,
}

/// Mints NGSI-LD URNs and scopes for expanded entities.
///
/// The generator is cloneable and thread-safe: the deduplication counter is a [`DashMap`] and the
/// resolver shares its templating engine through an `Arc`, so every pipeline worker can hold its own
/// clone without contending on a lock.
#[derive(Clone)]
pub struct UrnGenerator {
    /// Per-identifier counter backing the [`DeduplicationStrategy::Increment`] strategy.
    ///
    /// The keys are raw identifiers read out of the source data, so the map hashes with `ahash`:
    /// it resists a hostile key distribution without paying `SipHash` on every record.
    counter: DashMap<String, usize, RandomState>,
    /// The shared resolver used to evaluate identity, scope, and relationship templates.
    resolver: TemplateResolver,
}

impl UrnGenerator {
    /// Creates a generator over a shared template resolver.
    #[must_use]
    pub fn new(resolver: TemplateResolver) -> UrnGenerator {
        UrnGenerator {
            counter: DashMap::with_hasher(RandomState::new()),
            resolver,
        }
    }

    /// Generates the entity URN for one record under `mapping`.
    ///
    /// The deduplication strategy is chosen from the mapping's temporality and the staticness of its
    /// identity and attributes: temporal entities and dynamic identities are used directly, while a
    /// static identity paired with varying attributes is incremented so each record gets a distinct
    /// URN.
    ///
    /// # Errors
    ///
    /// Returns [`UrnError::GeneratedIdEmpty`] when the identity resolves to null, to empty text, or
    /// to text with no URN-safe character, and another [`UrnError`] when the identity template is
    /// missing or the built URN is malformed.
    pub fn generate_id(&self, mapping: &Mapping, data: &Value) -> Result<Urn> {
        let entity_type = mapping.data_model().entity_type();
        let template = mapping.identity().compiled_entity_name().as_ref().ok_or(UrnError::RelationshipMissingSource)?;
        // The identity resolves through the same definition of "the identifier a source produces" as
        // a relationship object does, so a null identity is absent rather than the text `null`.
        let raw_id = self.resolver.resolve_joined(slice::from_ref(template), data)?;

        match Self::determine_strategy(mapping) {
            DeduplicationStrategy::Direct => Self::generate_target_id(entity_type, &raw_id),
            DeduplicationStrategy::Increment => self.build_incrementing_urn(entity_type, &raw_id),
        }
    }

    /// Generates the scope, or scopes, for an entity from its mapping's `scope` declaration.
    ///
    /// A scope template that resolves to null or an empty string contributes no scope; a multi-scope
    /// declaration that yields nothing at all resolves to `None` rather than an empty list, and one
    /// whose templates resolve to the same scope yields it once.
    ///
    /// # Errors
    ///
    /// Returns [`UrnError`] when a scope template fails to resolve or its resolved text is not a
    /// valid NGSI-LD scope.
    pub fn generate_scope(&self, mapping: &Mapping, data: &Value) -> Result<Option<NgsiLdScope>> {
        let Some(compiled_scope) = mapping.compiled_scope() else {
            return Ok(None);
        };

        match compiled_scope {
            CompiledScope::Single(template) => match self.resolve_scope_text(template, data)? {
                Some(text) => Ok(Some(NgsiLdScope::from(Self::parse_scope(text)?))),
                None => Ok(None),
            },
            CompiledScope::Multiple(templates) => {
                let mut scopes = Vec::with_capacity(templates.len());
                for template in templates {
                    if let Some(text) = self.resolve_scope_text(template, data)? {
                        scopes.push(Self::parse_scope(text)?);
                    }
                }

                Ok(NgsiLdScope::from_scopes(scopes))
            }
        }
    }

    /// Generates the URN of the entity a relationship attribute points at.
    ///
    /// The target identifier comes from the attribute's compiled source, falling back to its raw
    /// source for a relationship declared as a bare literal.
    ///
    /// # Errors
    ///
    /// Returns [`UrnError`] when the relationship has no usable source, no target, or the built URN
    /// is malformed.
    pub fn generate_child_id(&self, attribute: &Attribute, data: &Value) -> Result<Urn> {
        let raw_id = match attribute.compiled_source() {
            Some(templates) => self.resolver.resolve_joined(templates, data)?,
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

        let target = attribute.target().as_ref().ok_or(UrnError::NoRelationshipTarget)?;
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
    pub fn generate_instance_child_id(&self, instance: &AttributeInstance, attribute: &Attribute, data: &Value) -> Result<Urn> {
        let target = attribute.target().as_ref().ok_or(UrnError::NoRelationshipTarget)?;
        let raw_id = match instance.compiled_source() {
            Some(templates) => self.resolver.resolve_joined(templates, data)?,
            None => return Err(UrnError::RelationshipMissingSource),
        };

        Self::generate_target_id(target.entity().entity_type(), &raw_id)
    }

    /// Mints the target URNs for one instance of a multi-attribute `ListRelationship`, one per token
    /// of that instance's `objectList` source that names a target.
    ///
    /// The instance source is tokenized exactly as a plain list relationship's is
    /// ([`resolve_tokens`](TemplateResolver::resolve_tokens)) and minted through the same
    /// [`mint_targets`](Self::mint_targets), so a token that names no target is dropped here too. An
    /// instance left with no token yields an empty vector; the caller records each object under its
    /// instance, so an empty instance is simply absent (ETSI GS CIM 009 v1.9.1 clause 4.5.5, EXAMPLE
    /// 19).
    ///
    /// # Errors
    ///
    /// Returns [`UrnError`] when the relationship declares no target, a source template fails to
    /// resolve, or a built URN is malformed.
    pub fn generate_instance_child_ids(&self, instance: &AttributeInstance, attribute: &Attribute, data: &Value) -> Result<Vec<Urn>> {
        let target = attribute.target().as_ref().ok_or(UrnError::NoRelationshipTarget)?;
        let tokens = match instance.compiled_source() {
            Some(templates) => self.resolver.resolve_tokens(templates, data)?,
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
    /// is dropped (see [`mint_targets`](Self::mint_targets)).
    ///
    /// # Errors
    /// Returns [`UrnError`] when the relationship declares no target, a source template fails to
    /// resolve, or a built URN is malformed.
    pub fn generate_child_ids(&self, attribute: &Attribute, data: &Value) -> Result<Vec<Urn>> {
        let target = attribute.target().as_ref().ok_or(UrnError::NoRelationshipTarget)?;

        Self::mint_targets(target.entity().entity_type(), &self.source_identifiers(attribute, data)?)
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
    pub fn source_identifiers(&self, attribute: &Attribute, data: &Value) -> Result<Vec<String>> {
        if let Some(templates) = attribute.compiled_source() {
            Ok(self.resolver.resolve_tokens(templates, data)?)
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

    /// Cleans a raw identifier into the URN's identifier segment, failing when nothing survives.
    fn segment(entity_type: &NameBuf, raw_id: &str) -> Result<IdSegment> {
        IdSegment::clean(raw_id).ok_or_else(|| UrnError::GeneratedIdEmpty {
            // The error owns the type name after the borrowed mapping or target is dropped.
            target_entity_type: entity_type.clone(),
        })
    }

    /// Resolves one scope template to its text, treating an absent field anywhere in the template,
    /// or an empty result, as "no scope".
    ///
    /// A scope path is only meaningful whole: `/{{ country }}/{{ city }}` over a record with no city
    /// must contribute no scope rather than `/Slovenia/null`, which is a legal but wrong scope.
    fn resolve_scope_text(&self, template: &CompiledTemplate, data: &Value) -> Result<Option<String>> {
        let text = match self.resolver.resolve_complete(template, data)? {
            Some(Value::String(text)) => text,
            None => return Ok(None),
            Some(other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_))) => other.to_string(),
        };

        if text.is_empty() { Ok(None) } else { Ok(Some(text)) }
    }

    /// Validates resolved scope text into an NGSI-LD scope.
    fn parse_scope(text: String) -> Result<ScopeBuf> {
        match ScopeBuf::new(&text) {
            Ok(scope) => Ok(scope),
            Err(source) => Err(UrnError::InvalidScope {
                rejected: text.into_boxed_str(),
                source,
            }),
        }
    }

    /// Chooses the deduplication strategy for `mapping`.
    fn determine_strategy(mapping: &Mapping) -> DeduplicationStrategy {
        // A temporal entity (one carrying `observedAt`) represents the same entity at successive
        // points in time, not distinct instances, so it must never receive a numeric suffix.
        if mapping.is_temporal() {
            return DeduplicationStrategy::Direct;
        }

        let identity_is_static = Analysis::is_identity_static(mapping.identity());
        let attributes_are_static = Analysis::is_attributes_static(mapping.attributes());

        match (identity_is_static, attributes_are_static) {
            // A constant identity with varying attributes needs a suffix to stay unique per record.
            (true, false) => DeduplicationStrategy::Increment,
            // A singleton (constant identity and attributes) or a dynamic identity (unique by
            // construction) is used directly.
            (true, true) | (false, true | false) => DeduplicationStrategy::Direct,
        }
    }

    /// Builds a URN with a per-identifier numeric suffix.
    ///
    /// The identifier is cleaned before the counter advances, so an empty identity fails without
    /// minting `<Type>:-1` and without consuming a count.
    fn build_incrementing_urn(&self, entity_type: &NameBuf, raw_id: &str) -> Result<Urn> {
        let segment = Self::segment(entity_type, raw_id)?;
        let count = {
            let mut count = self.counter.entry(raw_id.to_string()).or_insert(0);
            *count += 1;
            *count
        };

        UrnBuilder::build(entity_type.as_str(), &segment.with_suffix(count))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compiler::ExpanderCompiler,
        urn::{error::UrnError, generator::UrnGenerator},
    };
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use serde_json::{Value, json};
    use std::path::Path;

    /// Mints the identity of an `AircraftType` named by `identity` for one record.
    fn generate_aircraft_type_id(identity: &str, data: &Value) -> Result<String, UrnError> {
        let document = r#"{
            version: "v4",
            dataModel: "AircraftType",
            identity: { entityName: "IDENTITY" },
            attributes: { codeIATA: { source: "{{ code }}" } },
        }"#
        .replace("IDENTITY", identity);
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(&document, Path::new("test.json5"), &mut runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, &mut runner);
        let generator = UrnGenerator::new(runner.resolver());

        generator.generate_id(&mapping, data).map(|urn| urn.to_string())
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
    fn a_static_identity_cleaning_to_nothing_is_empty_rather_than_a_bare_counter() {
        let result = generate_aircraft_type_id("!?", &json!({"code": "E7W"}));

        assert!(is_generated_id_empty(&result), "{result:?}");
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
