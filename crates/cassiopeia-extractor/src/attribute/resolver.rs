use crate::attribute::{
    conversion::{self, readers::SourceParts},
    declaration_trail::{DeclarationTrail, Nesting},
    metadata::MetadataExtractor,
    refusal::AttributeRefusal,
    resolution_context::ResolutionContext,
    resolution_error::{ResolutionError, Result},
};
use cassiopeia_ir::{attribute_content::AttributeContent, resolved_attribute::ResolvedAttribute};
use cassiopeia_mapping::{
    attribute::{Attribute, instance::AttributeInstance},
    template::CompiledTemplate,
};
use cassiopeia_ngsi_ld::{
    entity::{
        attribute::{NgsiLdAttributeKind, language_property::LanguageMap},
        name::NameBuf,
    },
    value::{json_payload::JsonObject, omitted_nulls::OmittedNulls, types::Value},
};
use cassiopeia_unreadable_timestamps::timestamp_loss::TimestampLoss;
use serde_json::Value as JsonValue;
use smallvec::smallvec;
use std::result;

/// Resolves one attribute declaration into what it carries against the record, or `None` when it
/// carries nothing.
///
/// The declaration's [`NgsiLdAttributeKind`] and shape select the resolution path: a relationship
/// reads the entity's pre-resolved targets, a language property builds a `languageMap`, a
/// declaration carrying nested `mappings` builds a structured object, and everything else reads and
/// transforms source values.
pub(crate) fn resolve(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<ResolvedAttribute>> {
    if ctx.recursion_limit_exceeded() {
        return Err(ResolutionError::RecursionLimitExceeded { depth: ctx.depth });
    }

    let kind = config.kind();
    if matches!(kind, NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship) {
        resolve_relationship(ctx, attr_name, config)?;
        return Ok(None);
    }
    if let Some(instances) = config.instances() {
        return resolve_instances(ctx, attr_name, config, instances);
    }

    let content = if matches!(kind, NgsiLdAttributeKind::LanguageProperty) && !config.language_map().is_empty() {
        resolve_language_property(ctx, attr_name, config)?
    } else if !config.mappings().is_empty() {
        resolve_object(ctx, attr_name, config)?
    } else {
        resolve_leaf(ctx, attr_name, config)?
    };

    Ok(content.map(ResolvedAttribute::Single))
}

/// Resolves a sub-attribute's declaration into the one content it contributes: what a
/// single-instance declaration resolved to, or, for one declaring instances, a value listing every
/// instance that resolved.
pub(crate) fn resolve_content(ctx: &ResolutionContext, key: &NameBuf, config: &Attribute) -> Result<Option<AttributeContent>> {
    let mut child = ctx.child(ctx.data);
    Ok(match resolve(&mut child, key, config)? {
        None => None,
        Some(ResolvedAttribute::Single(content)) => Some(content),
        Some(ResolvedAttribute::Instances(instances)) => Some(AttributeContent::Value(Value::Array(
            instances.into_iter().flatten().filter_map(|content| into_value(ctx, key, content)).collect(),
        ))),
    })
}

/// Turns resolved content into an NGSI-LD Value, for an item of the list a sub-attribute's instances
/// make, where only a Value can stand.
///
/// Raw JSON becomes a Value without the nulls it holds, which are recorded: they were admissible in a
/// `JsonProperty`'s `json` member, but not inside a Property's value (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.0). A vocabulary IRI becomes its text, and a `languageMap` the object of text it is.
fn into_value(ctx: &ResolutionContext, key: &NameBuf, content: AttributeContent) -> Option<Value> {
    match content {
        AttributeContent::Value(value) => Some(value),
        other @ (AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_)) => {
            let mut omitted = OmittedNulls::default();
            let value = Value::from_json(JsonValue::from(other), &mut omitted);
            ctx.dropped.nulls.record(key, omitted);
            value
        }
    }
}

/// Resolves one member of an object nested `mappings` assemble into the raw JSON it contributes, or
/// `None` when it contributes nothing.
///
/// A member is read but not admitted by its own type: the assembled object is admitted once, by the
/// type of the attribute holding it, so a null a member keeps survives under a `JsonProperty` and is
/// left out, and counted under that attribute, under a `Property` (see [`conversion::read_member`]).
/// A member declaring instances contributes the list of the instances that resolved, a member
/// holding its own `mappings` the object they assemble, read as the member's conversion names, and
/// a relationship member contributes nothing, since its object travels on the entity.
fn resolve_member(ctx: &ResolutionContext, key: &NameBuf, config: &Attribute) -> Result<Option<JsonValue>> {
    let mut member = ctx.child(ctx.data);
    if member.recursion_limit_exceeded() {
        return Err(ResolutionError::RecursionLimitExceeded { depth: member.depth });
    }

    if matches!(config.kind(), NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship) {
        return Ok(None);
    }
    if let Some(instances) = config.instances() {
        let mut items = Vec::with_capacity(instances.len());
        for instance in instances {
            let parts = collect_source_parts(&member, instance.source().as_ref(), instance.compiled_source().as_ref())?;
            items.extend(read_member(&member, key, config, parts));
        }
        return Ok(if items.is_empty() { None } else { Some(JsonValue::Array(items)) });
    }

    if matches!(config.kind(), NgsiLdAttributeKind::LanguageProperty) && !config.language_map().is_empty() {
        return Ok(resolve_language_property(&mut member, key, config)?.map(JsonValue::from));
    }
    if !config.mappings().is_empty() {
        return Ok(assemble(&member, key, config)?.and_then(|object| read_member(&member, key, config, smallvec![JsonValue::Object(object)])));
    }

    let parts = collect_source_parts(&member, config.source().as_ref(), config.compiled_source().as_ref())?;
    Ok(read_member(&member, key, config, parts))
}

/// Reads one member's source values as the raw JSON it contributes, binding a refusal, and any nulls
/// left out of a value it built, to the sinks that gather them.
fn read_member(ctx: &ResolutionContext, key: &NameBuf, config: &Attribute, parts: SourceParts) -> Option<JsonValue> {
    convert(ctx, key, |omitted| {
        conversion::read_member(parts, config.conversion(), config.geometry().as_ref(), omitted)
    })
}

/// Assembles the object a declaration's nested `mappings` spell, member by member, as raw JSON, or
/// `None` when no member contributed anything.
fn assemble(ctx: &ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<JsonObject>> {
    let owner = DeclarationTrail::below(attr_name, Nesting::Member, ctx.owners);
    let members = ctx.beneath(&owner);
    let mut object = JsonObject::new();
    for (key, child_config) in config.mappings() {
        if let Some(member) = resolve_member(&members, key, child_config)? {
            object.insert(String::from(key.as_str()), member);
        }
    }

    Ok(if object.is_empty() { None } else { Some(object) })
}

/// Resolves a multi-instance Property-family attribute into one entry per declared instance.
///
/// Each instance's `source` is read and converted with the attribute's shared conversion; the entry
/// at index `i` aligns with the per-item metadata recorded for instance `i` (its `datasetId` and the
/// shared attribute-level properties). A missing instance stays `None` so the alignment holds, and
/// the transformer drops it when it builds the instances (ETSI GS CIM 009 v1.9.1 clause 4.5.5). When
/// no instance resolved, the whole attribute is omitted.
fn resolve_instances(
    ctx: &mut ResolutionContext,
    attr_name: &NameBuf,
    config: &Attribute,
    instances: &[AttributeInstance],
) -> Result<Option<ResolvedAttribute>> {
    let mut contents = Vec::with_capacity(instances.len());
    for instance in instances {
        let parts = collect_source_parts(ctx, instance.source().as_ref(), instance.compiled_source().as_ref())?;
        contents.push(transform(ctx, attr_name, config, parts));
    }

    record_metadata(ctx, attr_name, config)?;

    if contents.iter().all(Option::is_none) {
        Ok(None)
    } else {
        Ok(Some(ResolvedAttribute::Instances(contents)))
    }
}

/// Resolves a relationship attribute.
///
/// The relationship's targets were minted by the expander and travel on the entity, directly or
/// grouped per instance; the value itself is materialised later, so this only records the
/// attribute's own properties as metadata and contributes no entry to the value map.
fn resolve_relationship(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<()> {
    if ctx.relationships.has_objects(attr_name) {
        record_metadata(ctx, attr_name, config)?;
    }
    Ok(())
}

/// Resolves a `LanguageProperty` into a `languageMap` of per-language values.
///
/// A `languageMap` is keyed by BCP-47 language tag, not attribute name (ETSI GS CIM 009 v1.9.1
/// clause 4.5.18), and each entry is a single localized string, so every value is resolved as a
/// leaf against the same record rather than through the name-keyed nested-attribute path. An entry
/// that resolved to nothing, to blank text, or to anything but text has no text to localize and is
/// left out; when none is left, the attribute carries nothing.
fn resolve_language_property(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<AttributeContent>> {
    let mut language_map = LanguageMap::default();
    for (language, child_config) in config.language_map() {
        let parts = collect_source_parts(ctx, child_config.source().as_ref(), child_config.compiled_source().as_ref())?;
        if let Some(AttributeContent::Value(Value::String(text))) = transform(ctx, attr_name, child_config, parts)
            && !text.is_empty()
        {
            // The map owns its tags, which are only borrowed from the mapping's declaration.
            language_map.insert(language.clone(), String::from(text));
        }
    }

    record_metadata(ctx, attr_name, config)?;

    if language_map.is_empty() {
        Ok(None)
    } else {
        Ok(Some(AttributeContent::Language(language_map)))
    }
}

/// Resolves a declaration carrying nested `mappings` into a structured value.
///
/// The members assemble one raw JSON object (see [`assemble`]), which converts as the one part it
/// is through the declaration's [`Attribute::conversion`], exactly as a value read from one source
/// does: a declared `transformation` is applied as written, and naming none keeps the object as
/// assembled, or, on a `GeoProperty`, reads it as the geometry it spells field by field, under the
/// declaration's own `geometry` policy. The declaration's type admits the object once, so the nulls
/// its members kept survive under a `JsonProperty` and are left out, and counted under this
/// attribute, under a `Property`.
fn resolve_object(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<AttributeContent>> {
    let object = assemble(ctx, attr_name, config)?;
    record_metadata(ctx, attr_name, config)?;

    Ok(object.and_then(|object| transform(ctx, attr_name, config, smallvec![JsonValue::Object(object)])))
}

/// Resolves a leaf declaration: reads its source values and applies its transformation.
fn resolve_leaf(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<AttributeContent>> {
    let parts = collect_source_parts(ctx, config.source().as_ref(), config.compiled_source().as_ref())?;
    record_metadata(ctx, attr_name, config)?;

    Ok(transform(ctx, attr_name, config, parts))
}

/// Converts one declaration's source values into its content, binding a refusal, and any nulls
/// left out of the value, to the sinks that gather them.
///
/// Every path that converts an attribute's own value (a leaf, one multi-attribute instance, one
/// language-map entry, and the object nested `mappings` assemble) converts here, so all of them
/// convert through the declaration's [`Attribute::conversion`], and a declaration naming no
/// transformation gets its kind's default on every path.
fn transform(ctx: &ResolutionContext, attr_name: &NameBuf, config: &Attribute, parts: SourceParts) -> Option<AttributeContent> {
    convert(ctx, attr_name, |omitted| {
        conversion::apply(parts, config.conversion(), config.geometry().as_ref(), omitted)
    })
}

/// Runs one conversion of a declaration's value and binds a refusal, and any nulls left out of the
/// value, to the sinks that gather them.
///
/// The nulls left out are recorded only when the declaration keeps a value: a refused or absent
/// value lost the whole declaration, not some of its members.
fn convert<T>(
    ctx: &ResolutionContext,
    attr_name: &NameBuf,
    conversion: impl FnOnce(&mut OmittedNulls) -> result::Result<Option<T>, AttributeRefusal>,
) -> Option<T> {
    let mut omitted = OmittedNulls::default();
    let outcome = conversion(&mut omitted);

    let content = bind_refusal(ctx, attr_name, outcome)?;
    ctx.dropped.nulls.record(attr_name, omitted);
    Some(content)
}

/// Binds the outcome of converting one declaration's value to the sink that gathers its refusal.
///
/// A refusal is not a record failure: the attribute drops, the entity is still emitted, and the run
/// reports what was lost and why. Every conversion the resolver runs binds its outcome here, so none
/// of them can forget to.
fn bind_refusal<T>(ctx: &ResolutionContext, attr_name: &NameBuf, outcome: result::Result<Option<T>, AttributeRefusal>) -> Option<T> {
    match outcome {
        Ok(value) => value,
        Err(AttributeRefusal::Geometry(refusal)) => {
            ctx.dropped.geometries.record(attr_name, refusal);
            None
        }
        Err(AttributeRefusal::JsonPayload(refusal)) => {
            ctx.dropped.json_payloads.record(attr_name, refusal);
            None
        }
        Err(AttributeRefusal::UnreadableTimestamp { text }) => {
            ctx.dropped.timestamps.record(TimestampLoss::AttributeValue, attr_name, &text);
            None
        }
        Err(AttributeRefusal::UnusableValue { required, text }) => {
            ctx.dropped.unusable_values.record(ctx.path_of(attr_name), required, &text);
            None
        }
    }
}

/// Reads the raw source values a declaration is built from, one per template.
///
/// A single source string that is the lone reference `{{ context }}` binds the whole record;
/// otherwise each source template is evaluated against the record, or, when a source was written as
/// a literal that never compiled, taken verbatim. Taking the source and its compiled templates as
/// arguments lets both an [`Attribute`] and one of its [`AttributeInstance`]s be read through the
/// same path.
fn collect_source_parts(ctx: &ResolutionContext, source: Option<&JsonValue>, compiled: Option<&Vec<CompiledTemplate>>) -> Result<SourceParts> {
    // The compiled form, not the source text, decides: `{{context}}` and `{{ context }}` are the
    // same reference however they are spaced.
    if matches!(source, Some(JsonValue::String(_))) && matches!(compiled.map(Vec::as_slice), Some([CompiledTemplate::Simple(path)]) if path.is_whole_record()) {
        return Ok(smallvec![ctx.data.clone()]);
    }

    let mut parts = SourceParts::new();
    match source {
        Some(JsonValue::String(literal)) => match compiled.and_then(|templates| templates.first()) {
            Some(template) => parts.push(ctx.resolver.resolve(template, ctx.data)?),
            None => parts.push(JsonValue::String(literal.clone())),
        },
        Some(JsonValue::Array(sources)) => match compiled {
            Some(templates) => {
                for (index, _source) in sources.iter().enumerate() {
                    if let Some(template) = templates.get(index) {
                        parts.push(ctx.resolver.resolve(template, ctx.data)?);
                    }
                }
            }
            None => parts.extend(sources.iter().cloned()),
        },
        Some(other) => parts.push(other.clone()),
        None => {}
    }

    Ok(parts)
}

/// Extracts an attribute's properties as metadata and records them on the root context, if any.
///
/// Only the root context carries a metadata sink; a child context skips extraction entirely rather
/// than resolving a sub-tree it would discard. A sub-attribute's own sub-attributes are gathered
/// directly by the metadata extractor's recursive property-map resolution, so letting a child
/// re-extract here would duplicate that work at every level of nesting.
fn record_metadata(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<()> {
    if ctx.metadata.is_none() {
        return Ok(());
    }
    if let Some(storage) = MetadataExtractor::extract(ctx, attr_name, config)?
        && let Some(metadata) = &mut ctx.metadata
    {
        metadata.insert(attr_name.clone(), storage);
    }

    Ok(())
}
