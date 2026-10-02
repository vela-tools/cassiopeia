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
    /// Only `Expression` and `Complex` templates enter Tera; the other shapes are resolved by direct
    /// lookup, which is what keeps the common case out of the templating engine entirely. A
    /// `Composite` resolves to null when any field it interpolates is missing, null, or empty text,
    /// exactly as a lone `Simple` field reference over that field does.
    ///
    /// # Errors
    /// Returns [`TemplateError::Render`] when a Tera template fails to render, and
    /// [`TemplateError::Decode`] when an `Expression` template's rendering is not the JSON encoding
    /// of a value.
    pub fn resolve(&self, compiled: &CompiledTemplate, data: &JsonValue) -> Result<JsonValue> {
        match compiled {
            // The return type owns its `JsonValue`, so the literal is cloned into an owned string.
            CompiledTemplate::Static(literal) => Ok(JsonValue::String(literal.clone())),
            CompiledTemplate::Simple(key) => Ok(key.read(data)),
            CompiledTemplate::Composite(parts) => Ok(Self::join_complete(parts, data).map_or(JsonValue::Null, JsonValue::String)),
            CompiledTemplate::Expression(name) => self.evaluate(name, data),
            CompiledTemplate::Complex(name) => Ok(JsonValue::String(self.render(name, data)?)),
        }
    }

    /// Evaluates one compiled template, yielding `None` when the value it produces is absent.
    ///
    /// A value is absent when it is null or empty text, which is what
    /// [`resolve`](Self::resolve) yields for a missing field, a `Composite` with any field absent, a
    /// suppressed `Expression`, and a `Complex` template rendering nothing. Tera cannot report which
    /// fields a template read, so an `Expression` or `Complex` template is judged by its result alone.
    /// A static literal is never absent: it reads no field, so it is complete by construction.
    ///
    /// # Errors
    /// Returns the errors of [`resolve`](Self::resolve).
    pub fn resolve_complete(&self, compiled: &CompiledTemplate, data: &JsonValue) -> Result<Option<JsonValue>> {
        match compiled {
            // The return type owns its `JsonValue`, so the literal is cloned into an owned string.
            CompiledTemplate::Static(literal) => Ok(Some(JsonValue::String(literal.clone()))),
            CompiledTemplate::Simple(_) | CompiledTemplate::Composite(_) | CompiledTemplate::Expression(_) | CompiledTemplate::Complex(_) => {
                Ok(present(self.resolve(compiled, data)?))
            }
        }
    }

    /// Concatenates the resolved text of several templates into one identifier, or yields `None`
    /// when any template that reads the record resolves to an absent value.
    ///
    /// This is the single definition of "the identifier a source produces": the expander mints an
    /// entity's own URN, and the URN of every Relationship object, from it. An identifier is only
    /// meaningful whole, so a source declared as `["Airport-", "{{ id }}"]` over a record with no
    /// `id` names no entity rather than `Airport-`, which would be a well-formed RFC 8141 URN for an
    /// entity that does not exist and would merge every such record into it. Static parts read no
    /// field and never make the identifier absent.
    ///
    /// # Errors
    /// Returns the errors of [`resolve`](Self::resolve).
    pub fn resolve_joined(&self, templates: &[CompiledTemplate], data: &JsonValue) -> Result<Option<String>> {
        let mut combined = String::new();
        for template in templates {
            match self.resolve_complete(template, data)? {
                Some(value) => append_value(&mut combined, value),
                None => return Ok(None),
            }
        }

        Ok(Some(combined))
    }

    /// Resolves several templates and splits their combined output into identifier tokens.
    ///
    /// A resolved array contributes one token per element and a string one per whitespace- or
    /// comma-separated token, empty tokens dropped so they cannot mint an empty URN. An absent value,
    /// including a `Composite` with any field absent, contributes no token. The expander tokenizes a
    /// list relationship's source through this to mint one relationship object per token.
    ///
    /// # Errors
    /// Returns the errors of [`resolve`](Self::resolve).
    pub fn resolve_tokens(&self, templates: &[CompiledTemplate], data: &JsonValue) -> Result<Vec<String>> {
        let mut tokens = Vec::new();
        for template in templates {
            collect_identifiers(self.resolve(template, data)?, &mut tokens);
        }

        Ok(tokens)
    }

    /// Concatenates the parts of a composite template, stringifying any non-string field value, or
    /// yields `None` as soon as one field is absent (see [`present`]).
    ///
    /// This is the only way a composite is joined. Writing an absent field as the text `null`, or
    /// as nothing, would turn `Station-{{ id }}` over a record with no `id` into `Station-null` or
    /// `Station-`: a legal NGSI-LD entity id (ETSI GS CIM 009 v1.9.1 clause 4.5.1) that names an
    /// entity the source never described, into which every such record would merge.
    fn join_complete(parts: &[TemplatePart], data: &JsonValue) -> Option<String> {
        parts.iter().try_fold(String::with_capacity(Self::joined_capacity(parts)), |mut out, part| {
            match part {
                TemplatePart::Static(literal) => out.push_str(literal),
                TemplatePart::Dynamic(key) => append_value(&mut out, present(key.read(data))?),
            }
            Some(out)
        })
    }

    /// Sizes a composite's output buffer from its known static text plus a small allowance per
    /// dynamic field, so the common composite (a literal prefix and one field) fills without
    /// reallocating.
    fn joined_capacity(parts: &[TemplatePart]) -> usize {
        parts
            .iter()
            .map(|part| match part {
                TemplatePart::Static(literal) => literal.len(),
                TemplatePart::Dynamic(_) => 8,
            })
            .sum()
    }

    /// Renders an `Expression` template and decodes its rendering back into the expression's value.
    ///
    /// The template was registered in a form that encodes its one expression as JSON (see
    /// [`typed_form`](crate::template::value_expression::typed_form)), so the rendering is either that
    /// encoding, padded only by the whitespace around the guards, or nothing at all when a guard
    /// suppressed the expression; nothing is an absent value.
    fn evaluate(&self, name: &TemplateName, data: &JsonValue) -> Result<JsonValue> {
        let rendered = self.render(name, data)?;
        let encoded = rendered.trim();
        if encoded.is_empty() {
            return Ok(JsonValue::Null);
        }

        serde_json::from_str(encoded).map_err(|source| TemplateError::Decode {
            template: name.clone(),
            source,
        })
    }

    /// Renders a registered Tera template against a source record.
    ///
    /// Top-level fields are inserted as bare names and the whole record is also bound to `this`, so a
    /// field whose name is not a valid Tera identifier stays reachable as `this['CO(GT)']`. A
    /// positional (headerless) record is bound to `this` as an array instead, so a Tera expression
    /// can index it as `this[0]`; object key access, which the direct-lookup path uses, cannot be
    /// written inside a Tera expression.
    fn render(&self, name: &TemplateName, data: &JsonValue) -> Result<String> {
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

        self.tera.render(name.as_str(), &context).map_err(|source| TemplateError::Render {
            template: name.clone(),
            source,
        })
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

/// Keeps a resolved field value, or yields `None` when it is absent: null or empty text.
fn present(value: JsonValue) -> Option<JsonValue> {
    match value {
        JsonValue::Null => None,
        JsonValue::String(text) if text.is_empty() => None,
        other @ (JsonValue::String(_) | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_)) => Some(other),
    }
}

/// Appends one field value to a composite's output, a string as its text and anything else as its
/// JSON rendering.
fn append_value(out: &mut String, value: JsonValue) {
    match value {
        JsonValue::String(text) => out.push_str(&text),
        other @ (JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_)) => {
            out.push_str(&other.to_string());
        }
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
        TemplateSource,
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
    fn a_composite_with_every_field_present_resolves_to_its_concatenation() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Composite(vec![
            TemplatePart::Dynamic(FieldPath::new("first")),
            TemplatePart::Static(" ".to_string()),
            TemplatePart::Dynamic(FieldPath::new("last")),
        ]);

        assert_eq!(
            resolver.resolve(&compiled, &json!({"first": "John", "last": "Doe"})).unwrap(),
            json!("John Doe")
        );
    }

    #[test]
    fn a_composite_with_a_missing_null_or_empty_field_resolves_to_null_rather_than_the_text_null() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Composite(vec![TemplatePart::Static("Station-".to_string()), TemplatePart::Dynamic(FieldPath::new("id"))]);

        for record in [json!({}), json!({"id": null}), json!({"id": ""})] {
            assert_eq!(resolver.resolve(&compiled, &record).unwrap(), json!(null), "{record}");
        }
    }

    #[test]
    fn a_complete_composite_resolves_to_its_concatenation() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Composite(vec![
            TemplatePart::Static("/".to_string()),
            TemplatePart::Dynamic(FieldPath::new("country")),
            TemplatePart::Static("/".to_string()),
            TemplatePart::Dynamic(FieldPath::new("city")),
        ]);

        assert_eq!(
            resolver
                .resolve_complete(&compiled, &json!({"country": "Slovenia", "city": "Ljubljana"}))
                .unwrap(),
            Some(json!("/Slovenia/Ljubljana"))
        );
    }

    #[test]
    fn a_composite_with_a_missing_null_or_empty_field_is_incomplete() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Composite(vec![
            TemplatePart::Static("/".to_string()),
            TemplatePart::Dynamic(FieldPath::new("country")),
            TemplatePart::Static("/".to_string()),
            TemplatePart::Dynamic(FieldPath::new("city")),
        ]);

        for record in [
            json!({"country": "Slovenia"}),
            json!({"country": "Slovenia", "city": null}),
            json!({"country": "Slovenia", "city": ""}),
        ] {
            assert_eq!(resolver.resolve_complete(&compiled, &record).unwrap(), None, "{record}");
        }
    }

    #[test]
    fn a_complete_composite_stringifies_a_non_string_field() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Composite(vec![TemplatePart::Static("/Zone".to_string()), TemplatePart::Dynamic(FieldPath::new("zone"))]);

        assert_eq!(resolver.resolve_complete(&compiled, &json!({"zone": 7})).unwrap(), Some(json!("/Zone7")));
    }

    #[test]
    fn a_simple_template_over_an_absent_field_is_incomplete() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Simple(FieldPath::new("city"));

        assert_eq!(resolver.resolve_complete(&compiled, &json!({})).unwrap(), None);
        assert_eq!(resolver.resolve_complete(&compiled, &json!({"city": ""})).unwrap(), None);
        assert_eq!(
            resolver.resolve_complete(&compiled, &json!({"city": "/Ljubljana"})).unwrap(),
            Some(json!("/Ljubljana"))
        );
    }

    #[test]
    fn a_static_template_is_always_complete() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Static("/Ljubljana".to_string());

        assert_eq!(resolver.resolve_complete(&compiled, &json!({})).unwrap(), Some(json!("/Ljubljana")));
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

        assert_eq!(
            resolver.resolve_joined(&templates, &json!({"id": 535})).unwrap().as_deref(),
            Some("Airport-535")
        );
    }

    #[test]
    fn resolve_joined_of_a_lone_absent_field_is_absent_rather_than_the_text_null() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Simple(FieldPath::new("missing"))];

        assert_eq!(resolver.resolve_joined(&templates, &json!({})).unwrap(), None);
    }

    #[test]
    fn resolve_joined_is_absent_when_a_field_part_is_absent_rather_than_joining_the_static_parts() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Static("Airport-".to_string()), CompiledTemplate::Simple(FieldPath::new("id"))];

        for record in [json!({}), json!({"id": null}), json!({"id": ""})] {
            assert_eq!(resolver.resolve_joined(&templates, &record).unwrap(), None, "{record}");
        }
    }

    #[test]
    fn resolve_joined_joins_static_only_parts() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Static("Airport-".to_string()), CompiledTemplate::Static("LJU".to_string())];

        assert_eq!(resolver.resolve_joined(&templates, &json!({})).unwrap().as_deref(), Some("Airport-LJU"));
    }

    #[test]
    fn resolve_tokens_yields_no_token_for_a_composite_over_an_absent_field() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Composite(vec![
            TemplatePart::Static("Gate-".to_string()),
            TemplatePart::Dynamic(FieldPath::new("gate")),
        ])];

        assert!(resolver.resolve_tokens(&templates, &json!({})).unwrap().is_empty());
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
    fn a_suppressed_expression_is_incomplete() {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new("{% if city %}{{ city | upper }}{% endif %}")).unwrap();
        let resolver = runner.resolver();

        assert_eq!(resolver.resolve_complete(&compiled, &json!({"city": null})).unwrap(), None);
        assert_eq!(resolver.resolve_complete(&compiled, &json!({"city": "lj"})).unwrap(), Some(json!("LJ")));
    }

    #[test]
    fn resolve_tokens_yields_one_token_per_element_of_a_split_expression() {
        let mut runner = TemplateRunner::new();
        let templates = vec![runner.compile(&TemplateSource::new("{{ equipment | split(pat=';') }}")).unwrap()];
        let resolver = runner.resolver();

        assert_eq!(resolver.resolve_tokens(&templates, &json!({"equipment": "744;777"})).unwrap(), ["744", "777"]);
    }

    #[test]
    fn resolve_joined_writes_an_expression_array_as_compact_json() {
        let mut runner = TemplateRunner::new();
        let templates = vec![runner.compile(&TemplateSource::new("{{ codes | split(pat=' ') }}")).unwrap()];
        let resolver = runner.resolver();

        assert_eq!(
            resolver.resolve_joined(&templates, &json!({"codes": "BS IN"})).unwrap().as_deref(),
            Some(r#"["BS","IN"]"#)
        );
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
