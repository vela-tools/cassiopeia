use crate::urn::{
    analysis::Analysis,
    builder::UrnBuilder,
    cleaner::Cleaner,
    error::{Result, UrnError},
};
use ahash::RandomState;
use cassiopeia_mapping::{
    attribute::{Attribute, instance::AttributeInstance},
    identity::Identity,
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
    /// Returns [`UrnError`] when the identity template is missing, resolves to an empty identifier,
    /// or the built URN is malformed.
    pub fn generate_id(&self, mapping: &Mapping, data: &Value) -> Result<Urn> {
        let entity_type = mapping.data_model().entity_type();
        let strategy = Self::determine_strategy(mapping);

        let template = mapping.identity().compiled_entity_name().as_ref().ok_or(UrnError::RelationshipMissingSource)?;
        let raw_id = self.resolver.resolve(template, data)?.to_string();
        if raw_id.is_empty() {
            return Err(UrnError::GeneratedIdEmpty {
                // The error owns the type name after the borrowed mapping is dropped.
                target_entity_type: entity_type.clone(),
            });
        }
        let entity_type = entity_type.as_str();

        match strategy {
            DeduplicationStrategy::Direct => Self::build_direct_urn(entity_type, &raw_id),
            DeduplicationStrategy::Increment => self.build_incrementing_urn(entity_type, &raw_id),
        }
    }

    /// Generates the scope, or scopes, for an entity from its identity configuration.
    ///
    /// A scope template that resolves to null or an empty string contributes no scope; a multi-scope
    /// declaration that yields nothing at all resolves to `None` rather than an empty list.
    ///
    /// # Errors
    ///
    /// Returns [`UrnError`] when a scope template fails to resolve or its resolved text is not a
    /// valid NGSI-LD scope.
    pub fn generate_scope(&self, identity: &Identity, data: &Value) -> Result<Option<NgsiLdScope>> {
        let Some(compiled_scope) = identity.compiled_scope() else {
            return Ok(None);
        };

        match compiled_scope {
            CompiledScope::Single(template) => match self.resolve_scope_text(template, data)? {
                Some(text) => Ok(Some(NgsiLdScope::Single(Self::parse_scope(text)?))),
                None => Ok(None),
            },
            CompiledScope::Multiple(templates) => {
                let mut scopes = Vec::with_capacity(templates.len());
                for template in templates {
                    if let Some(text) = self.resolve_scope_text(template, data)? {
                        scopes.push(Self::parse_scope(text)?);
                    }
                }

                if scopes.is_empty() { Ok(None) } else { Ok(Some(NgsiLdScope::List(scopes))) }
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
        self.generate_target_id(target.entity().entity_type(), &raw_id)
    }

    /// Mints the target URN for one instance of a multi-attribute Relationship from that instance's
    /// own `source` template.
    ///
    /// Each instance of a multi-attribute Relationship (ETSI GS CIM 009 v1.9.1 clause 4.5.5) declares
    /// its own object-id source; every instance shares the attribute-level `target`. An instance
    /// whose source resolves to no identifier yields [`UrnError::GeneratedIdEmpty`], so the caller
    /// drops it in lockstep with the per-instance metadata the extractor records.
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

        self.generate_target_id(target.entity().entity_type(), &raw_id)
    }

    /// Mints the target URNs for one instance of a multi-attribute `ListRelationship`, one per token
    /// of that instance's `objectList` source.
    ///
    /// The instance source is tokenized exactly as a plain list relationship's is
    /// ([`resolve_tokens`](TemplateResolver::resolve_tokens)); an instance that tokenizes to nothing
    /// yields an empty vector, so the caller drops it in lockstep with its metadata (ETSI GS CIM 009
    /// v1.9.1 clause 4.5.5, EXAMPLE 19).
    ///
    /// # Errors
    ///
    /// Returns [`UrnError`] when the relationship declares no target, or a built URN is malformed.
    pub fn generate_instance_child_ids(&self, instance: &AttributeInstance, attribute: &Attribute, data: &Value) -> Result<Vec<Urn>> {
        let target = attribute.target().as_ref().ok_or(UrnError::NoRelationshipTarget)?;
        let entity_type = target.entity().entity_type();
        let tokens = match instance.compiled_source() {
            Some(templates) => self.resolver.resolve_tokens(templates, data)?,
            None => Vec::new(),
        };

        tokens.iter().map(|token| self.generate_target_id(entity_type, token)).collect()
    }

    /// Generates the URNs of every entity a list-relationship attribute points at.
    ///
    /// Unlike a single relationship, the source is read as a collection of identifiers: an array
    /// contributes one identifier per element, and a string contributes one per whitespace- or
    /// comma-separated token, so a single field holding several ids (a route's space-separated
    /// equipment codes, say) fans out into one relationship object each. Empty tokens are dropped.
    ///
    /// # Errors
    /// Returns [`UrnError`] when the relationship declares no target, or a built URN is malformed.
    pub fn generate_child_ids(&self, attribute: &Attribute, data: &Value) -> Result<Vec<Urn>> {
        let target = attribute.target().as_ref().ok_or(UrnError::NoRelationshipTarget)?;
        let entity_type = target.entity().entity_type();

        self.source_identifiers(attribute, data)?
            .iter()
            .map(|identifier| self.generate_target_id(entity_type, identifier))
            .collect()
    }

    /// Reads the identifiers an attribute's source contributes, without minting any URN.
    ///
    /// The tokenisation matches [`generate_child_ids`](Self::generate_child_ids): an array yields one
    /// identifier per element and a string one per whitespace- or comma-separated token, empty tokens
    /// dropped. A caller that fans an attribute out into one entity per identifier (a list
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
    /// Returns [`UrnError`] when `raw_id` is empty or the built URN is malformed.
    pub fn generate_target_id(&self, target_entity_type: &NameBuf, raw_id: &str) -> Result<Urn> {
        if raw_id.is_empty() {
            return Err(UrnError::GeneratedIdEmpty {
                // The error owns the type name after the borrowed target is dropped.
                target_entity_type: target_entity_type.clone(),
            });
        }

        let id = Cleaner::clean(raw_id);
        UrnBuilder::build(target_entity_type.as_str(), &id)
    }

    /// Resolves one scope template to its text, treating null and empty results as "no scope".
    fn resolve_scope_text(&self, template: &CompiledTemplate, data: &Value) -> Result<Option<String>> {
        let text = match self.resolver.resolve(template, data)? {
            Value::String(text) => text,
            Value::Null => return Ok(None),
            other @ (Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => other.to_string(),
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

    /// Builds a URN from a raw identifier with no deduplication.
    fn build_direct_urn(entity_type: &str, raw_id: &str) -> Result<Urn> {
        let id = Cleaner::clean(raw_id);
        UrnBuilder::build(entity_type, &id)
    }

    /// Builds a URN with a per-identifier numeric suffix.
    fn build_incrementing_urn(&self, entity_type: &str, raw_id: &str) -> Result<Urn> {
        let mut count = self.counter.entry(raw_id.to_string()).or_insert(0);
        *count += 1;

        let base_id = Cleaner::clean(raw_id);
        let final_id = if base_id.ends_with('-') {
            format!("{base_id}{}", *count)
        } else {
            format!("{base_id}-{}", *count)
        };

        UrnBuilder::build(entity_type, &final_id)
    }
}
