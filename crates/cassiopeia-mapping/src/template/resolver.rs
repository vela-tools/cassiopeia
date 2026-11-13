use crate::template::{
    CompiledTemplate,
    TemplatePart,
    error::{ResolutionFailure, Result},
    registered_template::RegisteredTemplate,
    render_diagnosis::diagnose,
    run_variables::VARIABLES,
    template_input::TemplateInput,
};
use serde_json::{Map, Value as JsonValue};
use std::sync::Arc;
use tera::{Context, Tera};

/// The variable a template reads the whole record through, so it can reach any field, one whose
/// name Tera cannot read as an identifier included.
pub(crate) const RECORD_VARIABLE: &str = "this";

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
    /// Returns a [`TemplateError`](crate::template::error::TemplateError) when a Tera template fails
    /// to render, or an `Expression` template's rendering is not the JSON encoding of a value; it
    /// names the template's declaration, and its [`ResolutionFailure`] names a field the record lacks
    /// or holds as null when that is what Tera failed on.
    pub fn resolve(&self, compiled: &CompiledTemplate, input: TemplateInput<'_>) -> Result<JsonValue> {
        match compiled {
            // The return type owns its `JsonValue`, so the literal is cloned into an owned string.
            CompiledTemplate::Static(literal) => Ok(JsonValue::String(literal.clone())),
            CompiledTemplate::Simple(key) => Ok(key.read(input)),
            CompiledTemplate::Composite(parts) => Ok(Self::join_complete(parts, input).map_or(JsonValue::Null, JsonValue::String)),
            CompiledTemplate::Expression(template) => self.evaluate(template, input),
            CompiledTemplate::Complex(template) => Ok(JsonValue::String(self.render(template, input)?)),
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
    pub fn resolve_complete(&self, compiled: &CompiledTemplate, input: TemplateInput<'_>) -> Result<Option<JsonValue>> {
        match compiled {
            // The return type owns its `JsonValue`, so the literal is cloned into an owned string.
            CompiledTemplate::Static(literal) => Ok(Some(JsonValue::String(literal.clone()))),
            CompiledTemplate::Simple(_) | CompiledTemplate::Composite(_) | CompiledTemplate::Expression(_) | CompiledTemplate::Complex(_) => {
                Ok(present(self.resolve(compiled, input)?))
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
    pub fn resolve_joined(&self, templates: &[CompiledTemplate], input: TemplateInput<'_>) -> Result<Option<String>> {
        let mut combined = String::new();
        for template in templates {
            match self.resolve_complete(template, input)? {
                Some(value) => append_value(&mut combined, value),
                None => return Ok(None),
            }
        }

        Ok(Some(combined))
    }

    /// Resolves the templates of a list-form source, one value per template in order, or yields
    /// `None` when the list reads the record and every template that reads it resolves to an absent
    /// value (see the private `present`).
    ///
    /// The values are what [`resolve`](Self::resolve) yields, absent ones included, for the
    /// declaration's conversion to merge: a source such as `["{{ street }}", " ", "{{ no }}"]` with
    /// only `no` present still joins to ` 7`. Only when nothing the list reads is there do its static
    /// parts stand alone, and they are then no value, whatever text they hold, so `["Station-",
    /// "{{ id }}"]` over a record with no `id` is absent rather than `Station-`. A list of static
    /// parts alone reads no field and resolves to them.
    ///
    /// # Errors
    /// Returns the errors of [`resolve`](Self::resolve).
    pub fn resolve_parts(&self, templates: &[CompiledTemplate], input: TemplateInput<'_>) -> Result<Option<Vec<JsonValue>>> {
        let mut values = Vec::with_capacity(templates.len());
        let mut reads = Reads::Nothing;
        for template in templates {
            let value = self.resolve(template, input)?;
            reads = match (reads, template) {
                (Reads::Present, _) | (_, CompiledTemplate::Static(_)) => reads,
                (Reads::Nothing | Reads::OnlyAbsent, _) if is_absent(&value) => Reads::OnlyAbsent,
                (Reads::Nothing | Reads::OnlyAbsent, _) => Reads::Present,
            };
            values.push(value);
        }

        Ok(match reads {
            Reads::Nothing | Reads::Present => Some(values),
            Reads::OnlyAbsent => None,
        })
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
    pub fn resolve_tokens(&self, templates: &[CompiledTemplate], input: TemplateInput<'_>) -> Result<Vec<String>> {
        let mut tokens = Vec::new();
        for template in templates {
            collect_identifiers(self.resolve(template, input)?, &mut tokens);
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
    fn join_complete(parts: &[TemplatePart], input: TemplateInput<'_>) -> Option<String> {
        parts.iter().try_fold(String::with_capacity(Self::joined_capacity(parts)), |mut out, part| {
            match part {
                TemplatePart::Static(literal) => out.push_str(literal),
                TemplatePart::Dynamic(key) => append_value(&mut out, present(key.read(input))?),
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
    fn evaluate(&self, template: &RegisteredTemplate, input: TemplateInput<'_>) -> Result<JsonValue> {
        let rendered = self.render(template, input)?;
        let encoded = rendered.trim();
        if encoded.is_empty() {
            return Ok(JsonValue::Null);
        }

        serde_json::from_str(encoded).map_err(|source| {
            template.fail(ResolutionFailure::Decode {
                // The failure outlives the mapping the template is borrowed from.
                template: template.source.clone(),
                source,
            })
        })
    }

    /// Renders a registered Tera template against a source record and its run variables.
    ///
    /// Top-level fields are inserted as bare names and the whole record is also bound to `this`, so a
    /// field whose name is not a valid Tera identifier stays reachable as `this['CO(GT)']`. The run
    /// variables are bound to `vars` after the fields, so `vars` always names them, even in a record
    /// with a field of that name, which stays reachable as `this.vars`. A positional (headerless)
    /// record is bound to `this` as an array instead, so a Tera expression can index it as `this[0]`;
    /// object key access, which the direct-lookup path uses, cannot be written inside a Tera
    /// expression.
    fn render(&self, template: &RegisteredTemplate, input: TemplateInput<'_>) -> Result<String> {
        let mut context = Context::new();
        let record = input.record();

        if let Some(fields) = record.as_object() {
            for (key, value) in fields {
                // The context owns its keys as `Cow<'static, str>`, so a borrowed field name has to
                // be cloned into it; the value is serialised in place and need not be owned.
                context.insert(key.clone(), value);
            }
            context.insert(VARIABLES, input.variables().as_value());

            if let Some(positional) = Self::positional_array(fields) {
                context.insert(RECORD_VARIABLE, &positional);
            } else {
                context.insert(RECORD_VARIABLE, record);
            }
        } else {
            context.insert(VARIABLES, input.variables().as_value());
            context.insert(RECORD_VARIABLE, record);
        }

        self.tera
            .render(template.name.as_str(), &context)
            .map_err(|engine| template.fail(diagnose(&self.tera, template, input, engine)))
    }

    /// Views a record as a positional array when its keys are exactly the contiguous zero-based
    /// indices `0..len`, as a headerless CSV source produces; otherwise returns `None`.
    ///
    /// The values are borrowed, not cloned: the returned slice serialises in place when Tera binds
    /// it to `this`.
    fn positional_array(fields: &Map<String, JsonValue>) -> Option<Vec<&JsonValue>> {
        if fields.is_empty() {
            return None;
        }

        let mut slots: Vec<Option<&JsonValue>> = vec![None; fields.len()];
        for (key, value) in fields {
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
    if is_absent(&value) { None } else { Some(value) }
}

/// Whether a resolved value is absent: null, or empty text.
const fn is_absent(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => true,
        JsonValue::String(text) => text.is_empty(),
        JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_) | JsonValue::Object(_) => false,
    }
}

/// What the templates of a list-form source resolved so far have found of the record.
#[derive(Clone, Copy)]
enum Reads {
    /// No template read the record: every one so far is static.
    Nothing,
    /// Every template that read the record resolved to an absent value.
    OnlyAbsent,
    /// A template that read the record resolved to a value.
    Present,
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
    use crate::{
        template::{
            CompiledTemplate,
            TemplatePart,
            TemplateSource,
            error::ResolutionFailure,
            field_path::FieldPath,
            registered_template::RegisteredTemplate,
            resolver::collect_identifiers,
            run_variables::RunVariables,
            runner::TemplateRunner,
            template_input::TemplateInput,
            template_name::TemplateName,
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value as JsonValue, json};
    use std::{
        path::Path,
        sync::{Arc, LazyLock},
    };

    /// The empty run variables every record is read with unless a test sets some.
    static NO_VARIABLES: LazyLock<RunVariables> = LazyLock::new(RunVariables::none);

    /// Reads `record` with no run variables.
    fn input(record: &JsonValue) -> TemplateInput<'_> {
        TemplateInput::new(record, &NO_VARIABLES)
    }

    /// The run variables `network`, set to `bikes`, and `districts`, a list.
    fn network_variables() -> RunVariables {
        let mut variables = Map::new();
        variables.insert("network".to_string(), json!("bikes"));
        variables.insert("districts".to_string(), json!(["Center", "Bežigrad"]));
        RunVariables::new(variables)
    }

    /// Compiles `source` and resolves it against `record` read with `variables`.
    fn resolve_with(source: &str, record: &JsonValue, variables: &RunVariables) -> JsonValue {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new(source), &location()).unwrap();
        runner.resolver().resolve(&compiled, TemplateInput::new(record, variables)).unwrap()
    }

    #[test]
    fn a_run_variable_is_read_from_vars_and_never_from_a_record_field_of_that_name() {
        let variables = network_variables();
        let record = json!({"id": 1, "vars": {"network": "from the source"}});

        assert_eq!(resolve_with("{{ vars.network }}", &record, &variables), json!("bikes"));
        assert_eq!(resolve_with("{{ vars.network | upper }}", &record, &variables), json!("BIKES"));
        assert_eq!(resolve_with("{{ vars.districts[0] }}", &record, &variables), json!("Center"));
        assert_eq!(resolve_with("{{ vars.network }}-{{ id }}", &record, &variables), json!("bikes-1"));
    }

    #[test]
    fn a_record_field_called_vars_is_read_through_this() {
        let variables = network_variables();
        let record = json!({"id": 1, "vars": {"network": "from the source"}});

        assert_eq!(resolve_with("{{ this['vars'] }}", &record, &variables), json!({"network": "from the source"}));
        assert_eq!(resolve_with("{{ this.vars.network | upper }}", &record, &variables), json!("FROM THE SOURCE"));
    }

    #[test]
    fn the_record_bound_to_this_holds_only_the_source_fields() {
        let variables = network_variables();
        let record = json!({"id": 1, "name": "Main station"});

        assert_eq!(resolve_with("{{ this | json_encode | json_decode }}", &record, &variables), record);
        assert_eq!(resolve_with("{{ this | length }}", &record, &variables), json!(2));
    }

    #[test]
    fn a_headerless_record_binds_this_to_its_columns_beside_the_run_variables() {
        let variables = network_variables();
        let record = json!({"0": "1", "1": "GKA"});

        assert_eq!(
            resolve_with("{{ this[1] | lower }}-{{ vars.network }}", &record, &variables),
            json!("gka-bikes")
        );
    }

    #[test]
    fn a_run_variable_no_level_sets_is_missing() {
        let record = json!({"id": 1});

        assert_eq!(resolve_with("{{ vars.network }}", &record, &NO_VARIABLES), JsonValue::Null);
    }

    fn location() -> TemplateLocation {
        TemplateLocation::new(
            Arc::from(Path::new("sensor.json5")),
            TemplateSite::Attribute(AttributePath::top_level(NameBuf::new("value").unwrap())),
        )
    }

    #[test]
    fn a_static_template_resolves_to_its_literal() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Static("Start".to_string());

        assert_eq!(resolver.resolve(&compiled, input(&json!({}))).unwrap(), json!("Start"));
    }

    #[test]
    fn a_simple_template_reads_the_named_field() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Simple(FieldPath::new("count"));

        assert_eq!(resolver.resolve(&compiled, input(&json!({"count": 7}))).unwrap(), json!(7));
    }

    #[test]
    fn a_composite_template_concatenates_and_stringifies() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Composite(vec![TemplatePart::Static("Station-".to_string()), TemplatePart::Dynamic(FieldPath::new("id"))]);

        assert_eq!(resolver.resolve(&compiled, input(&json!({"id": 42}))).unwrap(), json!("Station-42"));
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
            resolver.resolve(&compiled, input(&json!({"first": "John", "last": "Doe"}))).unwrap(),
            json!("John Doe")
        );
    }

    #[test]
    fn a_composite_with_a_missing_null_or_empty_field_resolves_to_null_rather_than_the_text_null() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Composite(vec![TemplatePart::Static("Station-".to_string()), TemplatePart::Dynamic(FieldPath::new("id"))]);

        for record in [json!({}), json!({"id": null}), json!({"id": ""})] {
            assert_eq!(resolver.resolve(&compiled, input(&record)).unwrap(), json!(null), "{record}");
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
                .resolve_complete(&compiled, input(&json!({"country": "Slovenia", "city": "Ljubljana"})))
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
            assert_eq!(resolver.resolve_complete(&compiled, input(&record)).unwrap(), None, "{record}");
        }
    }

    #[test]
    fn a_complete_composite_stringifies_a_non_string_field() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Composite(vec![TemplatePart::Static("/Zone".to_string()), TemplatePart::Dynamic(FieldPath::new("zone"))]);

        assert_eq!(resolver.resolve_complete(&compiled, input(&json!({"zone": 7}))).unwrap(), Some(json!("/Zone7")));
    }

    #[test]
    fn a_simple_template_over_an_absent_field_is_incomplete() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Simple(FieldPath::new("city"));

        assert_eq!(resolver.resolve_complete(&compiled, input(&json!({}))).unwrap(), None);
        assert_eq!(resolver.resolve_complete(&compiled, input(&json!({"city": ""}))).unwrap(), None);
        assert_eq!(
            resolver.resolve_complete(&compiled, input(&json!({"city": "/Ljubljana"}))).unwrap(),
            Some(json!("/Ljubljana"))
        );
    }

    #[test]
    fn a_static_template_is_always_complete() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Static("/Ljubljana".to_string());

        assert_eq!(resolver.resolve_complete(&compiled, input(&json!({}))).unwrap(), Some(json!("/Ljubljana")));
    }

    #[test]
    fn rendering_a_template_that_was_never_registered_is_an_error() {
        let resolver = TemplateRunner::new().resolver();
        let compiled = CompiledTemplate::Complex(RegisteredTemplate {
            name: TemplateName::for_source("{{ missing | upper }}"),
            source: TemplateSource::new("{{ missing | upper }}"),
            location: location(),
        });

        assert!(resolver.resolve(&compiled, input(&json!({}))).is_err());
    }

    #[test]
    fn a_failure_to_render_names_the_declaration_and_the_field_the_record_lacks() {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new("{{ n + 1 }}"), &location()).unwrap();

        let error = runner.resolver().resolve(&compiled, input(&json!({"id": "a"}))).unwrap_err();

        assert_eq!(error.location, location());
        assert!(matches!(error.failure.as_ref(), ResolutionFailure::MissingField { field, .. } if field.as_str() == "n"));
    }

    #[test]
    fn a_failure_to_render_a_printed_field_the_record_lacks_gives_the_missing_field_hint() {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new("{{ nothere }}-{{ id | lower }}"), &location()).unwrap();

        let error = runner.resolver().resolve(&compiled, input(&json!({"id": "A"}))).unwrap_err();

        assert_eq!(
            error.failure.to_string(),
            "`{{ nothere }}-{{ id | lower }}` reads `nothere`, which this record does not have: guard it with `{% if nothere %}…{% endif %}` or default it with `nothere | default(value=…)`"
        );
    }

    #[test]
    fn a_failure_to_render_a_printed_nested_field_the_record_lacks_gives_the_missing_field_hint() {
        let mut runner = TemplateRunner::new();
        let compiled = runner
            .compile(&TemplateSource::new("{{ address.city }}-{{ id | lower }}"), &location())
            .unwrap();

        let error = runner
            .resolver()
            .resolve(&compiled, input(&json!({"id": "A", "address": {"street": "Main"}})))
            .unwrap_err();

        assert_eq!(
            error.failure.to_string(),
            "`{{ address.city }}-{{ id | lower }}` reads `address.city`, which this record does not have: guard it with `{% if address.city %}…{% endif %}` or default it with `address.city | default(value=…)`"
        );
    }

    #[test]
    fn the_default_a_missing_nested_field_hint_suggests_resolves_on_the_same_record() {
        let mut runner = TemplateRunner::new();
        let failing = runner
            .compile(&TemplateSource::new("{{ address.street.name }}-{{ id | lower }}"), &location())
            .unwrap();
        let defaulted = runner
            .compile(
                &TemplateSource::new("{{ address.street | default(value='none') }}-{{ id | lower }}"),
                &location(),
            )
            .unwrap();
        let guarded = runner
            .compile(
                &TemplateSource::new("{% if address.street %}{{ address.street.name }}{% endif %}-{{ id | lower }}"),
                &location(),
            )
            .unwrap();
        let record = json!({"id": "A", "address": {}});
        let resolver = runner.resolver();

        let error = resolver.resolve(&failing, input(&record)).unwrap_err();

        assert!(matches!(error.failure.as_ref(), ResolutionFailure::MissingField { field, .. } if field.as_str() == "address.street"));
        assert_eq!(resolver.resolve(&defaulted, input(&record)).unwrap(), json!("none-a"));
        assert_eq!(resolver.resolve(&guarded, input(&record)).unwrap(), json!("-a"));
    }

    #[test]
    fn a_failure_on_a_null_nested_field_names_its_whole_path() {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new("{{ address.city | upper }}"), &location()).unwrap();

        let error = runner.resolver().resolve(&compiled, input(&json!({"address": {"city": null}}))).unwrap_err();

        assert!(matches!(error.failure.as_ref(), ResolutionFailure::NullField { field, .. } if field.as_str() == "address.city"));
    }

    #[test]
    fn a_failure_on_a_null_field_names_that_field() {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new("{{ code | split(pat=' ') }}"), &location()).unwrap();

        let error = runner.resolver().resolve(&compiled, input(&json!({"code": null}))).unwrap_err();

        assert!(matches!(error.failure.as_ref(), ResolutionFailure::NullField { field, .. } if field.as_str() == "code"));
    }

    #[test]
    fn a_guard_keeps_a_missing_field_from_failing_the_template() {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new("{% if n %}{{ n + 1 }}{% endif %}"), &location()).unwrap();

        assert_eq!(runner.resolver().resolve(&compiled, input(&json!({"id": "a"}))).unwrap(), JsonValue::Null);
        assert_eq!(runner.resolver().resolve(&compiled, input(&json!({"n": 1}))).unwrap(), json!(2));
    }

    #[test]
    fn resolve_joined_concatenates_and_stringifies_numbers() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Static("Airport-".to_string()), CompiledTemplate::Simple(FieldPath::new("id"))];

        assert_eq!(
            resolver.resolve_joined(&templates, input(&json!({"id": 535}))).unwrap().as_deref(),
            Some("Airport-535")
        );
    }

    #[test]
    fn resolve_joined_of_a_lone_absent_field_is_absent_rather_than_the_text_null() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Simple(FieldPath::new("missing"))];

        assert_eq!(resolver.resolve_joined(&templates, input(&json!({}))).unwrap(), None);
    }

    #[test]
    fn resolve_joined_is_absent_when_a_field_part_is_absent_rather_than_joining_the_static_parts() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Static("Airport-".to_string()), CompiledTemplate::Simple(FieldPath::new("id"))];

        for record in [json!({}), json!({"id": null}), json!({"id": ""})] {
            assert_eq!(resolver.resolve_joined(&templates, input(&record)).unwrap(), None, "{record}");
        }
    }

    /// The list-form source `["{{ street }}", separator, "{{ no }}"]`.
    fn address(separator: &str) -> Vec<CompiledTemplate> {
        vec![
            CompiledTemplate::Simple(FieldPath::new("street")),
            CompiledTemplate::Static(separator.to_string()),
            CompiledTemplate::Simple(FieldPath::new("no")),
        ]
    }

    #[test]
    fn resolve_parts_is_absent_when_no_field_the_list_reads_is_present_whatever_its_static_parts_hold() {
        let resolver = TemplateRunner::new().resolver();

        for separator in [" ", ", ", "Station-"] {
            for record in [json!({}), json!({"street": null, "no": ""}), json!({"street": "", "no": null})] {
                assert_eq!(
                    resolver.resolve_parts(&address(separator), input(&record)).unwrap(),
                    None,
                    "{separator:?} {record}"
                );
            }
        }
    }

    #[test]
    fn resolve_parts_keeps_every_value_absent_ones_included_when_one_field_is_present() {
        let resolver = TemplateRunner::new().resolver();

        assert_eq!(
            resolver.resolve_parts(&address(", "), input(&json!({"no": 7}))).unwrap(),
            Some(vec![JsonValue::Null, json!(", "), json!(7)])
        );
    }

    #[test]
    fn resolve_parts_of_static_parts_alone_resolves_to_them() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Static("N/A".to_string()), CompiledTemplate::Static(" ".to_string())];

        assert_eq!(
            resolver.resolve_parts(&templates, input(&json!({}))).unwrap(),
            Some(vec![json!("N/A"), json!(" ")])
        );
    }

    #[test]
    fn resolve_parts_counts_a_template_that_renders_its_own_fallback_as_present() {
        let mut runner = TemplateRunner::new();
        let source = TemplateSource::new("{% if street %}{{ street }}{% else %}unknown{% endif %}");
        let fallback = runner.compile(&source, &location()).unwrap();
        let resolver = runner.resolver();
        let templates = vec![
            fallback,
            CompiledTemplate::Static(" ".to_string()),
            CompiledTemplate::Simple(FieldPath::new("no")),
        ];

        assert_eq!(
            resolver.resolve_parts(&templates, input(&json!({}))).unwrap(),
            Some(vec![json!("unknown"), json!(" "), JsonValue::Null])
        );
    }

    #[test]
    fn resolve_joined_joins_static_only_parts() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Static("Airport-".to_string()), CompiledTemplate::Static("LJU".to_string())];

        assert_eq!(resolver.resolve_joined(&templates, input(&json!({}))).unwrap().as_deref(), Some("Airport-LJU"));
    }

    #[test]
    fn resolve_tokens_yields_no_token_for_a_composite_over_an_absent_field() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Composite(vec![
            TemplatePart::Static("Gate-".to_string()),
            TemplatePart::Dynamic(FieldPath::new("gate")),
        ])];

        assert_eq!(resolver.resolve_tokens(&templates, input(&json!({}))).unwrap(), [] as [String; 0]);
    }

    #[test]
    fn resolve_tokens_splits_a_delimited_source_and_drops_empties() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Simple(FieldPath::new("equipment"))];

        assert_eq!(
            resolver.resolve_tokens(&templates, input(&json!({"equipment": "744,  ,777"}))).unwrap(),
            ["744", "777"]
        );
    }

    #[test]
    fn resolve_tokens_yields_no_tokens_for_an_absent_source() {
        let resolver = TemplateRunner::new().resolver();
        let templates = vec![CompiledTemplate::Simple(FieldPath::new("missing"))];

        assert_eq!(resolver.resolve_tokens(&templates, input(&json!({}))).unwrap(), [] as [String; 0]);
    }

    #[test]
    fn a_suppressed_expression_is_incomplete() {
        let mut runner = TemplateRunner::new();
        let compiled = runner
            .compile(&TemplateSource::new("{% if city %}{{ city | upper }}{% endif %}"), &location())
            .unwrap();
        let resolver = runner.resolver();

        assert_eq!(resolver.resolve_complete(&compiled, input(&json!({"city": null}))).unwrap(), None);
        assert_eq!(resolver.resolve_complete(&compiled, input(&json!({"city": "lj"}))).unwrap(), Some(json!("LJ")));
    }

    #[test]
    fn resolve_tokens_yields_one_token_per_element_of_a_split_expression() {
        let mut runner = TemplateRunner::new();
        let templates = vec![runner.compile(&TemplateSource::new("{{ equipment | split(pat=';') }}"), &location()).unwrap()];
        let resolver = runner.resolver();

        assert_eq!(
            resolver.resolve_tokens(&templates, input(&json!({"equipment": "744;777"}))).unwrap(),
            ["744", "777"]
        );
    }

    #[test]
    fn resolve_joined_writes_an_expression_array_as_compact_json() {
        let mut runner = TemplateRunner::new();
        let templates = vec![runner.compile(&TemplateSource::new("{{ codes | split(pat=' ') }}"), &location()).unwrap()];
        let resolver = runner.resolver();

        assert_eq!(
            resolver.resolve_joined(&templates, input(&json!({"codes": "BS IN"}))).unwrap().as_deref(),
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

        assert_eq!(identifiers, [] as [String; 0]);
    }
}
