use crate::attribute::{
    conversion::{self, attribute_conversion::ConversionOutput, read_context::ReadContext, source_parts::SourceParts},
    declaration_output::DeclarationOutput,
    metadata::MetadataExtractor,
    refusal::AttributeRefusal,
    resolution_context::ResolutionContext,
    resolution_error::{ResolutionError, Result},
};
use cassiopeia_geometry::refusal::GeometryRefusal;
use cassiopeia_ir::{attribute_content::AttributeContent, resolved_attribute::ResolvedAttribute};
use cassiopeia_mapping::{
    attribute::{
        Attribute,
        declaration_trail::{DeclarationTrail, Nesting},
        instance::AttributeInstance,
    },
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

/// Resolves one top-level attribute declaration into what it carries against the record, or `None`
/// when it carries nothing.
///
/// Only a top-level attribute can declare instances: the mapping is refused at load time when any
/// declaration beneath one does (ETSI GS CIM 009 v1.9.1 clause 4.5.5). A relationship's instances
/// were minted by the expander and travel on the entity, so it resolves like any other relationship.
pub(crate) fn resolve(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<ResolvedAttribute>> {
    match (config.kind(), config.instances()) {
        (NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship, _) | (_, None) => resolve_declaration(ctx, attr_name, config),
        (_, Some(instances)) => resolve_instances(ctx, attr_name, config, instances),
    }
}

/// Resolves a sub-attribute's declaration into the one content it contributes.
pub(crate) fn resolve_content(ctx: &ResolutionContext, key: &NameBuf, config: &Attribute) -> Result<Option<AttributeContent>> {
    resolve_declaration(&mut ctx.child(ctx.data), key, config)
}

/// Resolves one declaration into what it produces where it sits (see [`DeclarationOutput`]), or
/// `None` when it carries nothing.
///
/// The declaration's [`NgsiLdAttributeKind`] and shape select the resolution path: a relationship
/// reads the entity's pre-resolved targets, a language property declaring a `languageMap` builds it entry by entry, a declaration carrying
/// nested `mappings` builds a structured object, and everything else reads and converts source
/// values.
fn resolve_declaration<O: DeclarationOutput>(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<O>> {
    if ctx.recursion_limit_exceeded() {
        return Err(ResolutionError::RecursionLimitExceeded { depth: ctx.depth });
    }

    let kind = config.kind();
    if matches!(kind, NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship) {
        resolve_relationship(ctx, attr_name, config)?;
        return Ok(None);
    }
    let content = if matches!(kind, NgsiLdAttributeKind::LanguageProperty) && !config.language_map().is_empty() {
        resolve_language_property(ctx, attr_name, config)?.map(|map| O::Content::from(AttributeContent::Language(map)))
    } else if !config.mappings().is_empty() {
        resolve_object(ctx, attr_name, config)?
    } else {
        resolve_leaf(ctx, attr_name, config)?
    };

    Ok(content.map(O::single))
}

/// Assembles the object a declaration's nested `mappings` spell, member by member, as raw JSON, or
/// `None` when no member contributed anything.
///
/// A member is read but not admitted by its own type: the assembled object is admitted once, by the
/// type of the attribute holding it, so a null a member keeps survives under a `JsonProperty` and is
/// left out, and counted under that attribute, under a `Property`. A relationship member contributes
/// nothing, since its object travels on the entity.
fn assemble(ctx: &ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<JsonObject>> {
    let owner = DeclarationTrail::below(attr_name, Nesting::Member, ctx.owners);
    let members = ctx.beneath(&owner);
    let mut object = JsonObject::new();
    for (key, child_config) in config.mappings() {
        if let Some(member) = resolve_declaration::<JsonValue>(&mut members.child(members.data), key, child_config)? {
            object.insert(String::from(key.as_str()), member);
        }
    }

    Ok(if object.is_empty() { None } else { Some(object) })
}

/// Resolves a top-level attribute declaring instances into one entry per declared instance, or
/// `None` when no instance resolved.
///
/// Each instance's `source` is read and converted with the attribute's shared conversion. An
/// instance that resolved to nothing stays `None`, so the entry at index `i` stays aligned with the
/// per-item metadata recorded for instance `i`: its `datasetId` and the shared attribute-level
/// properties (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
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
        return Ok(None);
    }

    Ok(Some(ResolvedAttribute::Instances(contents)))
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
fn resolve_language_property(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<LanguageMap>> {
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

    Ok(if language_map.is_empty() { None } else { Some(language_map) })
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
fn resolve_object<C: ConversionOutput>(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<C>> {
    let object = assemble(ctx, attr_name, config)?;
    record_metadata(ctx, attr_name, config)?;

    Ok(object.and_then(|object| transform(ctx, attr_name, config, smallvec![JsonValue::Object(object)])))
}

/// Resolves a leaf declaration: reads its source values and applies its transformation.
fn resolve_leaf<C: ConversionOutput>(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Option<C>> {
    let parts = collect_source_parts(ctx, config.source().as_ref(), config.compiled_source().as_ref())?;
    record_metadata(ctx, attr_name, config)?;

    Ok(transform(ctx, attr_name, config, parts))
}

/// Converts one declaration's source values into what they produce where the declaration sits,
/// binding a refusal, and any nulls left out of the value, to the sinks that gather them.
///
/// Every path that converts a declaration's own value (a leaf, one instance, one language-map
/// entry, and the object nested `mappings` assemble) converts here, so all of them convert through
/// the declaration's [`Attribute::conversion`], and a declaration naming no transformation gets its
/// kind's default on every path. The declaration's `geometry` policy is settled here, once: one it
/// declares none of is lossless-only.
///
/// The nulls left out are recorded only when the declaration keeps a value: a refused or absent
/// value lost the whole declaration, not some of its members.
fn transform<C: ConversionOutput>(ctx: &ResolutionContext, attr_name: &NameBuf, config: &Attribute, parts: SourceParts) -> Option<C> {
    let mut omitted = OmittedNulls::default();
    let outcome = conversion::convert(
        parts,
        config.conversion(),
        &mut ReadContext {
            policy: config.geometry().unwrap_or_default(),
            omitted: &mut omitted,
        },
    );

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
        Err(AttributeRefusal::Geometry(GeometryRefusal::Inadmissible(refusal))) => {
            ctx.dropped.geometries.record(ctx.path_of(attr_name), refusal);
            None
        }
        Err(AttributeRefusal::Geometry(GeometryRefusal::OutsideWgs84(position))) => {
            ctx.dropped.out_of_range_geometries.record(ctx.path_of(attr_name), &position);
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
