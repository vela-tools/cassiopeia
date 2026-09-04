use crate::{
    attribute::{
        metadata::MetadataExtractor,
        refusal::AttributeRefusal,
        resolution_context::ResolutionContext,
        transformer::{SourceParts, Transformer},
    },
    error::{ExtractionError, Result},
};
use cassiopeia_geometry::policy::GeometryPolicy;
use cassiopeia_mapping::{
    attribute::{Attribute, instance::AttributeInstance},
    template::CompiledTemplate,
    transformation::Transformation,
};
use cassiopeia_ngsi_ld::{
    entity::{attribute::NgsiLdAttributeKind, name::NameBuf},
    value::types::{Value, ValueObject},
};
use compact_str::CompactString;
use serde_json::Value as JsonValue;
use smallvec::smallvec;

/// Resolves one attribute declaration into its NGSI-LD value.
///
/// The declaration's [`NgsiLdAttributeKind`] and shape select the resolution path: a relationship
/// reads the entity's pre-resolved targets, a language property builds a `languageMap`, a
/// declaration carrying nested `mappings` builds a structured object, and everything else reads and
/// transforms source values.
pub(crate) fn resolve(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Value> {
    if ctx.recursion_limit_exceeded() {
        return Err(ExtractionError::RecursionLimitExceeded { depth: ctx.depth });
    }

    let kind = config.kind();
    if matches!(kind, NgsiLdAttributeKind::Relationship | NgsiLdAttributeKind::ListRelationship) {
        return resolve_relationship(ctx, attr_name, config);
    }
    if let Some(instances) = config.instances() {
        return resolve_instances(ctx, attr_name, config, instances);
    }
    if matches!(kind, NgsiLdAttributeKind::LanguageProperty) && !config.language_map().is_empty() {
        return resolve_language_property(ctx, attr_name, config);
    }
    if !config.mappings().is_empty() {
        return resolve_object(ctx, attr_name, config);
    }

    resolve_leaf(ctx, attr_name, config)
}

/// Resolves a multi-instance Property-family attribute into an array of per-instance values.
///
/// Each instance's `source` is read and transformed with the attribute's shared `transformation`;
/// the value at index `i` aligns with the per-item metadata recorded for instance `i` (its
/// `datasetId` and the shared attribute-level properties). A missing instance value stays null so the
/// alignment holds, and the transformer drops it when it builds the instances (ETSI GS CIM 009
/// v1.9.1 clause 4.5.5). When every instance is null the whole attribute is omitted.
fn resolve_instances(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute, instances: &[AttributeInstance]) -> Result<Value> {
    let mut values = Vec::with_capacity(instances.len());
    for instance in instances {
        let parts = collect_source_parts(ctx, instance.source().as_ref(), instance.compiled_source().as_ref())?;
        values.push(transform(ctx, attr_name, config, parts));
    }

    record_metadata(ctx, attr_name, config)?;

    if values.iter().all(Value::is_null) {
        Ok(Value::Null)
    } else {
        Ok(Value::Array(values))
    }
}

/// Resolves a relationship attribute.
///
/// The relationship's targets were minted by the resolve stage and travel on the entity; the value
/// itself is materialised later, so this only records the attribute's own properties as metadata and
/// contributes no entry to the value map.
fn resolve_relationship(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Value> {
    let has_targets = ctx.relationships.get(attr_name).is_some_and(|targets| !targets.is_empty());
    if !has_targets {
        return Ok(Value::Null);
    }

    record_metadata(ctx, attr_name, config)?;
    Ok(Value::Null)
}

/// Resolves a `LanguageProperty` into a `languageMap` of per-language values.
///
/// A `languageMap` is keyed by BCP-47 language tag, not attribute name (ETSI GS CIM 009 v1.9.1
/// clause 4.5.18), and each entry is a single localized string, so every value is resolved as a
/// leaf against the same record rather than through the name-keyed nested-attribute path.
fn resolve_language_property(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Value> {
    let mut language_map = ValueObject::default();
    for (language, child_config) in config.language_map() {
        let parts = collect_source_parts(ctx, child_config.source().as_ref(), child_config.compiled_source().as_ref())?;
        let value = transform(ctx, attr_name, child_config, parts);
        if !value.is_null() && !value.is_empty_string() {
            language_map.insert(CompactString::from(language.as_str()), value);
        }
    }

    record_metadata(ctx, attr_name, config)?;

    if language_map.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(Value::Object(Box::new(language_map)))
    }
}

/// Resolves a declaration carrying nested `mappings` into a structured object value.
fn resolve_object(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Value> {
    let mut object = ValueObject::default();
    for (key, child_config) in config.mappings() {
        let value = resolve_child(ctx, key, child_config)?;
        if !value.is_null() {
            object.insert(CompactString::from(key.as_str()), value);
        }
    }

    record_metadata(ctx, attr_name, config)?;

    if object.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(Value::Object(Box::new(object)))
    }
}

/// Resolves a leaf declaration: reads its source values and applies its transformation.
fn resolve_leaf(ctx: &mut ResolutionContext, attr_name: &NameBuf, config: &Attribute) -> Result<Value> {
    let parts = collect_source_parts(ctx, config.source().as_ref(), config.compiled_source().as_ref())?;
    record_metadata(ctx, attr_name, config)?;

    Ok(transform(ctx, attr_name, config, parts))
}

/// Transforms one declaration's source values, binding a refusal to the sink that gathers it.
///
/// A refusal is not a record failure: the attribute drops, the entity is still emitted, and the run
/// reports what was lost and why. The three paths that read source values (a leaf, one multi-attribute
/// instance, and one language-map entry) all bind their outcome here, so none of them can forget to.
fn transform(ctx: &ResolutionContext, attr_name: &NameBuf, config: &Attribute, parts: SourceParts) -> Value {
    let transformation: Option<&Transformation> = config.transformation().as_ref();
    let geometry: Option<&GeometryPolicy> = config.geometry().as_ref();

    match Transformer::apply(parts, transformation, geometry) {
        Ok(value) => value,
        Err(AttributeRefusal::Geometry(refusal)) => {
            ctx.dropped_geometries.record(attr_name, refusal);
            Value::Null
        }
        Err(AttributeRefusal::UnreadableTimestamp { text }) => {
            ctx.unreadable_timestamps.record(attr_name, &text);
            Value::Null
        }
    }
}

/// Resolves a nested declaration in a fresh child context.
fn resolve_child(ctx: &ResolutionContext, key: &NameBuf, config: &Attribute) -> Result<Value> {
    let mut child = ctx.child(ctx.data);
    resolve(&mut child, key, config)
}

/// Reads the raw source values a declaration is built from, one per template.
///
/// The literal `{{ context }}` binds the whole record; otherwise each source template is evaluated
/// against the record, or, when a source was written as a literal that never compiled, taken
/// verbatim. Taking the source and its compiled templates as arguments lets both an [`Attribute`] and
/// one of its [`AttributeInstance`]s be read through the same path.
fn collect_source_parts(ctx: &ResolutionContext, source: Option<&JsonValue>, compiled: Option<&Vec<CompiledTemplate>>) -> Result<SourceParts> {
    if matches!(source, Some(JsonValue::String(source)) if source == "{{ context }}") {
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
