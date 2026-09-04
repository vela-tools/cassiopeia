use crate::template::{
    CompiledTemplate,
    TemplatePart,
    error::{Result, TemplateError},
    template_name::TemplateName,
};
use serde_json::{Map, Value as JsonValue};
use std::sync::Arc;
use tera::{Context, Tera};

/// A lock-free view over a compiled Tera engine, used to evaluate templates per record.
///
/// Cloning shares the engine through an `Arc`, so every pipeline worker can hold its own resolver
/// without contending on a lock in the hot loop.
#[derive(Clone)]
pub struct TemplateResolver {
    tera: Arc<Tera>,
}

impl TemplateResolver {
    /// Builds a resolver over an already-populated engine.
    pub(crate) const fn new(tera: Arc<Tera>) -> TemplateResolver {
        TemplateResolver { tera }
    }

    /// Evaluates one compiled template against a source record.
    ///
    /// Only `Complex` templates enter Tera; the other shapes are resolved by direct lookup, which
    /// is what keeps the common case out of the templating engine entirely.
    ///
    /// # Errors
    /// Returns [`TemplateError::Render`] when a `Complex` template fails to render in Tera.
    pub fn resolve(&self, compiled: &CompiledTemplate, data: &JsonValue) -> Result<JsonValue> {
        match compiled {
            // The return type owns its `JsonValue`, so the literal is cloned into an owned string.
            CompiledTemplate::Static(literal) => Ok(JsonValue::String(literal.clone())),
            CompiledTemplate::Simple(key) => Ok(key.read(data)),
            CompiledTemplate::Composite(parts) => Ok(JsonValue::String(Self::join(parts, data))),
            CompiledTemplate::Complex(name) => self.render(name, data),
        }
    }

    /// Concatenates the resolved text of several templates into one identifier, dropping any part
    /// that resolves to null.
    ///
    /// This is the single definition of "the identifier a source produces". The relationship URN
    /// minted in the expander and the per-instance metadata compaction in the extractor both resolve
    /// a source through this, so a relationship instance survives in one stage exactly when it
    /// survives in the other (ETSI GS CIM 009 v1.9.1 clause 4.5.5). Unlike [`join`](Self::join), a
    /// null part contributes nothing rather than the text `null`, so an absent field yields an empty
    /// identifier the caller can drop.
    ///
    /// # Errors
    /// Returns [`TemplateError::Render`] when a `Complex` template fails to render in Tera.
    pub fn resolve_joined(&self, templates: &[CompiledTemplate], data: &JsonValue) -> Result<String> {
        let mut combined = String::new();
        for template in templates {
            match self.resolve(template, data)? {
                JsonValue::String(text) => combined.push_str(&text),
                JsonValue::Null => {}
                other @ (JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_)) => {
                    combined.push_str(&other.to_string());
                }
            }
        }

        Ok(combined)
    }

    /// Resolves several templates and splits their combined output into identifier tokens.
    ///
    /// A resolved array contributes one token per element and a string one per whitespace- or
    /// comma-separated token, empty tokens dropped so they cannot mint an empty URN. The expander
    /// (minting one relationship object per token) and the extractor (grouping a list relationship's
    /// instances) tokenize through this, so a list relationship's objects and its per-instance
    /// metadata split on the very same tokens.
    ///
    /// # Errors
    /// Returns [`TemplateError::Render`] when a `Complex` template fails to render in Tera.
    pub fn resolve_tokens(&self, templates: &[CompiledTemplate], data: &JsonValue) -> Result<Vec<String>> {
        let mut tokens = Vec::new();
        for template in templates {
            collect_identifiers(self.resolve(template, data)?, &mut tokens);
        }

        Ok(tokens)
    }

    /// Concatenates the parts of a composite template, stringifying any non-string field value.
    fn join(parts: &[TemplatePart], data: &JsonValue) -> String {
        // Seed the buffer from the known static text plus a small allowance per dynamic field, so the
        // common composite (a literal prefix and one field) fills without reallocating.
        let capacity = parts
            .iter()
            .map(|part| match part {
                TemplatePart::Static(literal) => literal.len(),
                TemplatePart::Dynamic(_) => 8,
            })
            .sum();
        parts.iter().fold(String::with_capacity(capacity), |mut out, part| {
            match part {
                TemplatePart::Static(literal) => out.push_str(literal),
                TemplatePart::Dynamic(key) => match key.read(data) {
                    JsonValue::String(value) => out.push_str(&value),
                    other @ (JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_)) => {
                        out.push_str(&other.to_string());
                    }
                },
            }
            out
        })
    }

    /// Renders a registered Tera template against a source record.
    ///
    /// Top-level fields are inserted as bare names and the whole record is also bound to `this`, so a
    /// field whose name is not a valid Tera identifier stays reachable as `this['CO(GT)']`. A
    /// positional (headerless) record is bound to `this` as an array instead, so a Tera expression
    /// can index it as `this[0]`; object key access, which the direct-lookup path uses, cannot be
    /// written inside a Tera expression.
    fn render(&self, name: &TemplateName, data: &JsonValue) -> Result<JsonValue> {
        let mut context = Context::new();

        if let Some(fields) = data.as_object() {
            for (key, value) in fields {
                // The context owns its keys as `Cow<'static, str>`, so a borrowed field name has to
                // be cloned into it; the value is serialised in place and need not be owned.
                context.insert(key.clone(), value);
            }

            if let Some(positional) = Self::positional_array(fields) {
                context.insert("this", &positional);
            } else {
                context.insert("this", data);
            }
        } else {
            context.insert("this", data);
        }

        let rendered = self.tera.render(name.as_str(), &context).map_err(|source| TemplateError::Render {
            template: name.clone(),
            source,
        })?;

        Ok(JsonValue::String(rendered))
    }

    /// Views a record as a positional array when its keys are exactly the contiguous zero-based
    /// indices `0..len`, as a headerless CSV source produces; otherwise returns `None`.
    ///
    /// The reserved `vars` key (run-level variables injected into every record) is not a source
    /// column, so it neither disqualifies an otherwise-positional record nor occupies a slot: a
    /// headerless record still binds `this` to its column array with `vars` reachable separately.
    ///
    /// The values are borrowed, not cloned: the returned slice serialises in place when Tera binds
    /// it to `this`.
    fn positional_array(fields: &Map<String, JsonValue>) -> Option<Vec<&JsonValue>> {
        let columns = fields.len() - usize::from(fields.contains_key("vars"));
        if columns == 0 {
            return None;
        }

        let mut slots: Vec<Option<&JsonValue>> = vec![None; columns];
        for (key, value) in fields {
            if key == "vars" {
                continue;
            }
            let index: usize = key.parse().ok()?;
            let slot = slots.get_mut(index)?;
            if slot.is_some() {
                return None;
            }
            *slot = Some(value);
        }

        slots.into_iter().collect()
    }
}

/// Appends the identifiers a resolved source contributes: every element of an array (recursively),
/// and every whitespace- or comma-separated token of a string. Empty tokens are skipped so they
/// cannot mint an empty URN; a number contributes its text; null, booleans, and objects contribute
/// nothing.
///
/// Exposed for the expander's uncompiled-source fallback, which tokenizes a raw literal source that
/// never went through the resolver; every compiled path goes through
/// [`resolve_tokens`](TemplateResolver::resolve_tokens) instead.
pub fn collect_identifiers(value: JsonValue, identifiers: &mut Vec<String>) {
    match value {
        JsonValue::Array(elements) => {
            for element in elements {
                collect_identifiers(element, identifiers);
            }
        }
        JsonValue::String(text) => {
            identifiers.extend(
                text.split(is_identifier_separator)
                    .map(str::trim)
                    .filter(|token| !token.is_empty())
                    .map(str::to_string),
            );
        }
        JsonValue::Number(number) => identifiers.push(number.to_string()),
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Object(_) => {}
    }
}

/// Whether `character` separates identifiers in a delimited list-relationship source.
const fn is_identifier_separator(character: char) -> bool {
    character.is_whitespace() || character == ','
}

#[cfg(test)]
mod tests {
    use crate::template::{
        CompiledTemplate,
        TemplatePart,
        field_path::FieldPath,
        resolver::collect_identifiers,
        runner::TemplateRunner,
        template_name::TemplateName,
    };
    use serde_json::json;

    #[test]
    fn a_static_template_resolves_to_its_literal() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Static("Start".to_string());

        assert_eq!(resolver.resolve(&compiled, &json!({})).unwrap(), json!("Start"));
    }

    #[test]
    fn a_simple_template_reads_the_named_field() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Simple(FieldPath::new("count"));

        assert_eq!(resolver.resolve(&compiled, &json!({"count": 7})).unwrap(), json!(7));
    }

    #[test]
    fn a_composite_template_concatenates_and_stringifies() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Composite(vec![TemplatePart::Static("Station-".to_string()), TemplatePart::Dynamic(FieldPath::new("id"))]);

        assert_eq!(resolver.resolve(&compiled, &json!({"id": 42})).unwrap(), json!("Station-42"));
    }

    #[test]
    fn rendering_a_template_that_was_never_registered_is_an_error() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Complex(TemplateName::for_source("{{ missing | upper }}"));

        assert!(resolver.resolve(&compiled, &json!({})).is_err());
    }

    #[test]
    fn resolve_joined_concatenates_and_stringifies_numbers() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Static("Airport-".to_string()), CompiledTemplate::Simple(FieldPath::new("id"))];

        assert_eq!(resolver.resolve_joined(&templates, &json!({"id": 535})).unwrap(), "Airport-535");
    }

    #[test]
    fn resolve_joined_skips_a_null_part_rather_than_writing_null() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Simple(FieldPath::new("missing"))];

        assert_eq!(resolver.resolve_joined(&templates, &json!({})).unwrap(), "");
    }

    #[test]
    fn resolve_tokens_splits_a_delimited_source_and_drops_empties() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Simple(FieldPath::new("equipment"))];

        assert_eq!(
            resolver.resolve_tokens(&templates, &json!({"equipment": "744,  ,777"})).unwrap(),
            ["744", "777"]
        );
    }

    #[test]
    fn resolve_tokens_yields_no_tokens_for_an_absent_source() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Simple(FieldPath::new("missing"))];

        assert!(resolver.resolve_tokens(&templates, &json!({})).unwrap().is_empty());
    }

    #[test]
    fn an_array_source_yields_one_identifier_per_element() {
        let mut identifiers = Vec::new();
        collect_identifiers(json!(["744", "777", 320]), &mut identifiers);

        assert_eq!(identifiers, ["744", "777", "320"]);
    }

    #[test]
    fn a_delimited_string_source_splits_into_identifiers() {
        let mut identifiers = Vec::new();
        collect_identifiers(json!("744 777"), &mut identifiers);

        assert_eq!(identifiers, ["744", "777"]);
    }

    #[test]
    fn empty_tokens_between_separators_are_skipped() {
        let mut identifiers = Vec::new();
        collect_identifiers(json!("744,  ,777"), &mut identifiers);

        assert_eq!(identifiers, ["744", "777"]);
    }

    #[test]
    fn a_null_source_yields_no_identifiers() {
        let mut identifiers = Vec::new();
        collect_identifiers(json!(null), &mut identifiers);

        assert!(identifiers.is_empty());
    }
}
