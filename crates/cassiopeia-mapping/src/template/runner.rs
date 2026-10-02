use crate::template::{
    CompiledTemplate,
    TemplateSource,
    compile_error::TemplateCompileError,
    contrib,
    direct_form,
    filter,
    function,
    resolver::TemplateResolver,
    template_name::TemplateName,
    value_expression,
};
use std::sync::Arc;
use tera::Tera;

/// Compiles mapping template expressions and owns the Tera engine the complex ones register with.
///
/// Compilation happens once, at mapping load time; the resulting `TemplateResolver` is what the
/// per-record path uses.
#[derive(Clone)]
pub struct TemplateRunner {
    tera: Arc<Tera>,
}

impl Default for TemplateRunner {
    fn default() -> TemplateRunner {
        TemplateRunner::new()
    }
}

impl TemplateRunner {
    /// Builds a runner with every Cassiopeia filter registered.
    #[must_use]
    pub fn new() -> TemplateRunner {
        let mut tera = Tera::default();
        filter::register(&mut tera);
        function::register(&mut tera);
        contrib::register(&mut tera);

        TemplateRunner { tera: Arc::new(tera) }
    }

    /// Hands out a lock-free resolver sharing this runner's engine.
    ///
    /// Call once every template has been compiled: templates registered afterwards are visible to
    /// resolvers taken later, not to ones already handed out.
    #[must_use]
    pub fn resolver(&self) -> TemplateResolver {
        TemplateResolver::new(Arc::clone(&self.tera))
    }

    /// Classifies one template expression and, when it needs Tera, registers it with the engine.
    ///
    /// # Errors
    /// Returns a [`TemplateCompileError`] when the expression needs Tera and Tera will not register
    /// it as written: a syntax error, such as an unclosed `{{`, `{%`, or `{#`, or a filter,
    /// function, or test that is not registered.
    pub fn compile(&mut self, source: &TemplateSource) -> Result<CompiledTemplate, TemplateCompileError> {
        let text = source.as_str();

        // Literals, lone field references, and concatenations of the two resolve without Tera on
        // the per-record path; only what that grammar rejects is registered with the engine.
        if let Some(direct) = direct_form::compile(text) {
            return Ok(direct);
        }

        let name = TemplateName::for_source(text);
        let tera = Arc::make_mut(&mut self.tera);
        // Registering the same expression twice is not an error: the name is a digest of the
        // expression, so a repeat registration replaces an identical template. The typed form is
        // derived from the source alone, so a digest of the source still names it uniquely.
        if let Some(typed) = value_expression::typed_form(text)
            && tera.add_raw_template(name.as_str(), &typed).is_ok()
        {
            return Ok(CompiledTemplate::Expression(name));
        }
        // A source the typed rewrite does not apply to, or whose rewrite Tera will not parse,
        // renders as written: to text. The rewrite can fail where the source parses, since its
        // added parentheses count against Tera's expression nesting limit, so only registering
        // the source as written decides whether the expression is valid. A failed registration
        // leaves the engine as it was.
        tera.add_raw_template(name.as_str(), text).map_err(|cause| TemplateCompileError {
            // The error outlives the mapping the expression is borrowed from.
            template: source.clone(),
            source: cause,
        })?;

        Ok(CompiledTemplate::Complex(name))
    }
}

#[cfg(test)]
mod tests {
    use crate::template::{CompiledTemplate, TemplatePart, TemplateSource, compile_error::TemplateCompileError, runner::TemplateRunner};
    use serde_json::{Value as JsonValue, json};
    use std::error::Error;

    fn compile(source: &str) -> (TemplateRunner, CompiledTemplate) {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new(source)).unwrap();

        (runner, compiled)
    }

    fn compile_error(source: &str) -> TemplateCompileError {
        TemplateRunner::new().compile(&TemplateSource::new(source)).unwrap_err()
    }

    /// An expression wrapped in `depth` pairs of parentheses, filtered so it takes the Tera path.
    fn parenthesised(depth: usize) -> String {
        format!("{{{{ {}a | upper{} }}}}", "(".repeat(depth), ")".repeat(depth))
    }

    fn resolve(source: &str, data: &JsonValue) -> JsonValue {
        let (runner, compiled) = compile(source);

        runner.resolver().resolve(&compiled, data).unwrap()
    }

    #[test]
    fn an_expression_without_interpolation_compiles_to_a_literal() {
        let (_, compiled) = compile("Start");

        assert!(matches!(compiled, CompiledTemplate::Static(literal) if literal == "Start"));
    }

    #[test]
    fn a_lone_field_reference_compiles_to_a_direct_lookup() {
        let (_, compiled) = compile("{{ id }}");

        assert!(matches!(compiled, CompiledTemplate::Simple(key) if key.as_str() == "id"));
    }

    #[test]
    fn a_prefixed_field_reference_compiles_to_a_concatenation() {
        let (_, compiled) = compile("Station-{{ id }}");

        assert!(matches!(compiled, CompiledTemplate::Composite(parts) if parts.len() == 2));
    }

    #[test]
    fn a_lone_filtered_expression_compiles_to_a_typed_tera_template() {
        let (_, compiled) = compile("{{ name | upper }}");

        assert!(matches!(compiled, CompiledTemplate::Expression(_)));
    }

    #[test]
    fn a_guarded_expression_compiles_to_a_typed_tera_template() {
        let (_, compiled) = compile("{% if name %}{{ name | upper }}{% endif %}");

        assert!(matches!(compiled, CompiledTemplate::Expression(_)));
    }

    #[test]
    fn a_conditional_with_literal_text_compiles_to_a_textual_tera_template() {
        let (_, compiled) = compile("{% if name %}{{ name }}{% else %}none{% endif %}");

        assert!(matches!(compiled, CompiledTemplate::Complex(_)));
    }

    #[test]
    fn a_split_expression_resolves_to_an_array() {
        assert_eq!(
            resolve(r#"{{ this[2] | split(pat=" ") }}"#, &json!({"0": "India", "1": "IN", "2": "BS IN"})),
            json!(["BS", "IN"])
        );
        assert_eq!(
            resolve(r#"{{ this[2] | split(pat=" ") }}"#, &json!({"0": "United Kingdom", "1": "GB", "2": "UK"})),
            json!(["UK"])
        );
    }

    #[test]
    fn a_json_decode_expression_resolves_to_the_decoded_value() {
        assert_eq!(resolve("{{ codes | json_decode }}", &json!({"codes": r#"["BS","IN"]"#})), json!(["BS", "IN"]));
        assert_eq!(resolve("{{ payload | json_decode }}", &json!({"payload": r#"{"a":1}"#})), json!({"a": 1}));
    }

    #[test]
    fn a_tera_template_mixing_text_and_an_expression_still_resolves_to_a_string() {
        assert_eq!(resolve("Station-{{ id | upper }}", &json!({"id": "a1"})), json!("Station-A1"));
    }

    #[test]
    fn a_suppressed_guard_resolves_to_null() {
        let source = r#"{% if this[2] %}{{ this[2] | split(pat=" ") }}{% endif %}"#;

        assert_eq!(resolve(source, &json!({"0": "Bonaire", "1": "BQ", "2": null})), JsonValue::Null);
        assert_eq!(resolve(source, &json!({"0": "Bonaire", "1": "BQ", "2": ""})), JsonValue::Null);
        assert_eq!(resolve(source, &json!({"0": "India", "1": "IN", "2": "BS IN"})), json!(["BS", "IN"]));
    }

    #[test]
    fn an_arithmetic_expression_keeps_its_numeric_type() {
        assert_eq!(resolve("{{ a | int + b | int }}", &json!({"a": "1", "b": "2"})), json!(3));
    }

    #[test]
    fn a_concatenation_operator_is_encoded_whole() {
        // Without the parentheses the encoding filter would bind to `b | upper` alone.
        assert_eq!(resolve("{{ a ~ '-' ~ b | upper }}", &json!({"a": "x", "b": "y"})), json!("x-Y"));
    }

    #[test]
    fn a_bracketed_field_name_does_not_force_the_tera_path() {
        let (_, compiled) = compile("{{ this['CO(GT)'] }}");

        assert!(matches!(compiled, CompiledTemplate::Simple(key) if key.as_str() == "CO(GT)"));
    }

    #[test]
    fn a_positional_accessor_compiles_to_a_direct_lookup() {
        let (_, compiled) = compile("{{ this[4] }}");

        assert!(matches!(compiled, CompiledTemplate::Simple(key) if key.as_str() == "4"));
    }

    #[test]
    fn a_positional_accessor_reads_the_indexed_column() {
        assert_eq!(resolve("{{ this[2] }}", &json!({"0": "a", "1": "b", "2": "c"})), json!("c"));
    }

    #[test]
    fn a_conditional_reads_a_positional_column_through_tera() {
        let data = json!({"0": "1", "1": "GKA"});

        assert_eq!(resolve("{% if this[1] %}{{ this[1] }}{% else %}none{% endif %}", &data), json!("GKA"));
    }

    #[test]
    fn a_bare_var_reference_reads_the_injected_vars_map() {
        let data = json!({"id": 1, "vars": {"valid_from": "2026-08-04T16:00:00Z"}});

        assert_eq!(resolve("{{ vars.valid_from }}", &data), json!("2026-08-04T16:00:00Z"));
    }

    #[test]
    fn a_filtered_var_reference_resolves_through_tera() {
        let data = json!({"id": 1, "vars": {"name": "senlab"}});

        assert_eq!(resolve("{{ vars.name | upper }}", &data), json!("SENLAB"));
    }

    #[test]
    fn the_reserved_vars_key_does_not_disturb_positional_this_access() {
        // A headerless record carrying injected vars still binds `this` to its column array, so a
        // Tera positional expression reads the column rather than tripping over the extra key.
        let data = json!({"0": "1", "1": "GKA", "vars": {"x": "y"}});

        assert_eq!(resolve("{% if this[1] %}{{ this[1] }}{% else %}none{% endif %}", &data), json!("GKA"));
    }

    #[test]
    fn a_literal_resolves_to_itself_regardless_of_the_record() {
        assert_eq!(resolve("Start", &json!({})), json!("Start"));
    }

    #[test]
    fn a_direct_lookup_preserves_the_source_value_type() {
        assert_eq!(resolve("{{ count }}", &json!({"count": 7})), json!(7));
    }

    #[test]
    fn a_direct_lookup_of_a_missing_field_resolves_to_null() {
        assert_eq!(resolve("{{ missing }}", &json!({})), JsonValue::Null);
    }

    #[test]
    fn a_concatenation_stringifies_non_string_field_values() {
        assert_eq!(resolve("Station-{{ id }}", &json!({"id": 42})), json!("Station-42"));
    }

    #[test]
    fn a_concatenation_reads_a_nested_path() {
        let data = json!({"properties": {"code": "SI"}});

        assert_eq!(resolve("code:{{ properties.code }}", &data), json!("code:SI"));
    }

    #[test]
    fn a_tera_expression_applies_its_filter() {
        assert_eq!(resolve("{{ name | upper }}", &json!({"name": "sensor"})), json!("SENSOR"));
    }

    #[test]
    fn a_tera_expression_reaches_a_bracketed_field_name() {
        let data = json!({"CO(GT)": " 2.6 "});

        assert_eq!(resolve("{{ this['CO(GT)'] | trim }}", &data), json!("2.6"));
    }

    #[test]
    fn the_clean_filter_is_registered() {
        let data = json!({"name": "Main   Street "});

        assert_eq!(resolve("{{ name | clean }}", &data), json!("Main Street"));
    }

    #[test]
    fn compiling_the_same_expression_twice_yields_the_same_template_name() {
        let mut runner = TemplateRunner::new();
        let first = runner.compile(&TemplateSource::new("{{ name | upper }}")).unwrap();
        let second = runner.compile(&TemplateSource::new("{{ name | upper }}")).unwrap();

        match (first, second) {
            (CompiledTemplate::Expression(first), CompiledTemplate::Expression(second)) => assert_eq!(first, second),
            other => panic!("expected two expression templates, got {other:?}"),
        }
    }

    #[test]
    fn a_resolver_taken_after_compilation_can_render_the_registered_template() {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new("{{ value | upper }}")).unwrap();
        let resolver = runner.resolver();

        assert_eq!(resolver.resolve(&compiled, &json!({"value": "x"})).unwrap(), json!("X"));
    }

    #[test]
    fn rendering_an_expression_over_a_missing_field_is_an_error() {
        let (runner, compiled) = compile("{{ missing | upper }}");

        assert!(runner.resolver().resolve(&compiled, &json!({})).is_err());
    }

    #[test]
    fn an_unclosed_expression_is_a_compile_error_naming_the_template_and_chaining_the_engine_report() {
        let error = compile_error("{{ a | upper ");

        assert_eq!(error.template, TemplateSource::new("{{ a | upper "));
        assert!(error.to_string().contains("{{ a | upper "));
        assert!(
            error
                .source()
                .expect("the engine report is chained")
                .to_string()
                .contains("Unexpected end of input")
        );
    }

    #[test]
    fn an_unclosed_conditional_is_a_compile_error() {
        // The typed rewrite applies to this shape and fails too; the error reports the source as
        // written, not the rewrite.
        let error = compile_error("{% if a %}{{ a | upper }}");

        assert_eq!(error.template, TemplateSource::new("{% if a %}{{ a | upper }}"));
        assert!(
            error
                .source()
                .expect("the engine report is chained")
                .to_string()
                .contains("{% if a %}{{ a | upper }}")
        );
    }

    #[test]
    fn an_unregistered_filter_is_a_compile_error() {
        let error = compile_error("{{ a | no_such_filter }}");

        assert!(error.source().expect("the engine report is chained").to_string().contains("no_such_filter"));
    }

    #[test]
    fn an_expression_whose_typed_rewrite_exceeds_the_nesting_limit_compiles_as_written() {
        // Tera allows 40 nested expression levels. The expression body sits one level in, so 39
        // pairs of parentheses parse as written while the rewrite's extra pair does not.
        let source = parenthesised(39);
        let (runner, compiled) = compile(&source);

        assert!(matches!(compiled, CompiledTemplate::Complex(_)));
        assert_eq!(runner.resolver().resolve(&compiled, &json!({"a": "x"})).unwrap(), json!("X"));
    }

    #[test]
    fn an_expression_past_the_nesting_limit_as_written_is_a_compile_error() {
        assert_eq!(compile_error(&parenthesised(40)).template, TemplateSource::new(parenthesised(40)));
    }

    #[test]
    fn a_loop_compiles_to_a_textual_tera_template_that_renders_every_iteration() {
        let source = "{% for x in xs %}{{ x }} {% endfor %}";
        let (_, compiled) = compile(source);

        assert!(matches!(compiled, CompiledTemplate::Complex(_)));
        assert_eq!(resolve(source, &json!({"xs": ["a", "b"]})), json!("a b "));
    }

    #[test]
    fn a_conditional_written_without_spaces_compiles_to_a_typed_tera_template() {
        let source = "{%if a%}{{ a }}{%endif%}";
        let (_, compiled) = compile(source);

        assert!(matches!(compiled, CompiledTemplate::Expression(_)));
        assert_eq!(resolve(source, &json!({"a": "x"})), json!("x"));
    }

    #[test]
    fn an_assignment_compiles_to_a_typed_tera_template() {
        let source = "{% set x = a %}{{ x }}";
        let (_, compiled) = compile(source);

        assert!(matches!(compiled, CompiledTemplate::Expression(_)));
        assert_eq!(resolve(source, &json!({"a": 5})), json!(5));
    }

    #[test]
    fn a_comment_beside_a_field_reference_compiles_to_a_typed_tera_template() {
        let source = "{# note #}{{ a }}";
        let (_, compiled) = compile(source);

        assert!(matches!(compiled, CompiledTemplate::Expression(_)));
        assert_eq!(resolve(source, &json!({"a": 5})), json!(5));
    }

    #[test]
    fn an_unfiltered_arithmetic_expression_resolves_to_a_number() {
        let (_, compiled) = compile("{{ a + 1 }}");

        assert!(matches!(compiled, CompiledTemplate::Expression(_)));
        assert_eq!(resolve("{{ a + 1 }}", &json!({"a": 2})), json!(3));
    }

    #[test]
    fn an_unfiltered_concatenation_compiles_to_a_typed_tera_template() {
        let (_, compiled) = compile("{{ a ~ '-' ~ b }}");

        assert!(matches!(compiled, CompiledTemplate::Expression(_)));
        assert_eq!(resolve("{{ a ~ '-' ~ b }}", &json!({"a": "x", "b": "y"})), json!("x-y"));
    }

    #[test]
    fn literal_expressions_compile_to_typed_tera_templates_yielding_the_literal() {
        assert_eq!(resolve(r#"{{ "lit" }}"#, &json!({"lit": "field"})), json!("lit"));
        assert_eq!(resolve("{{ 42 }}", &json!({})), json!(42));
        assert_eq!(resolve("{{ true }}", &json!({"true": "field"})), json!(true));
        assert_eq!(resolve("{{ none }}", &json!({"none": "field"})), JsonValue::Null);
    }

    #[test]
    fn a_negation_compiles_to_a_typed_tera_template() {
        let (_, compiled) = compile("{{ not a }}");

        assert!(matches!(compiled, CompiledTemplate::Expression(_)));
        assert_eq!(resolve("{{ not a }}", &json!({"a": false})), json!(true));
    }

    #[test]
    fn expressions_that_are_not_a_plain_field_reference_take_the_tera_path() {
        for source in [
            "{{ a * 2 }}",
            "{{ a == b }}",
            "{{ a . b }}",
            "{{ a['b'] }}",
            "{{ this }}",
            "{{ this.a }}",
            r#"{{ this["a"] }}"#,
            "{{ this['a.b'] }}",
            "{{- a -}}",
            "{{ __tera_context }}",
        ] {
            let (_, compiled) = compile(source);

            assert!(
                matches!(compiled, CompiledTemplate::Expression(_) | CompiledTemplate::Complex(_)),
                "{source}: {compiled:?}"
            );
        }
    }

    #[test]
    fn a_field_reference_reached_through_tera_reads_what_a_direct_lookup_would() {
        let data = json!({"a": {"b": 1}, "this": "shadowed", "a.b": "flat"});

        assert_eq!(resolve("{{ a . b }}", &data), json!(1));
        assert_eq!(resolve("{{ a['b'] }}", &data), json!(1));
        assert_eq!(resolve("{{ this.a.b }}", &data), json!(1));
        assert_eq!(resolve("{{ this['a.b'] }}", &data), json!("flat"));
        assert_eq!(resolve("{{- a.b -}}", &data), json!(1));
    }

    #[test]
    fn a_direct_lookup_reads_what_tera_reads_for_the_same_reference() {
        let named = json!({
            "and": 1, "in": 2, "is": 3, "or": 4, "loop": 5, "self": 6, "context": 7,
            "a": {"not": 8, "none": 9, "b": {"c": 10}}, "CO(GT)": 11, "": 12, "vars": {"x": 13},
        });
        let positional = json!({"0": "zero", "1": "one", "7": "seven", "2": "two", "3": "three", "4": "four", "5": "five", "6": "six"});

        for (reference, data) in [
            ("and", &named),
            ("in", &named),
            ("is", &named),
            ("or", &named),
            ("loop", &named),
            ("self", &named),
            ("context", &named),
            ("a.not", &named),
            ("a.none", &named),
            ("a.b.c", &named),
            ("this['CO(GT)']", &named),
            ("this['']", &named),
            ("vars.x", &named),
            ("this[1]", &positional),
            ("this[007]", &positional),
        ] {
            let (_, direct) = compile(&format!("{{{{ {reference} }}}}"));
            assert!(matches!(direct, CompiledTemplate::Simple(_)), "{reference}: {direct:?}");

            // The identity filter pair forces the same reference through Tera.
            let through_tera = format!("{{{{ {reference} | json_encode | json_decode }}}}");
            assert_eq!(resolve(&format!("{{{{ {reference} }}}}"), data), resolve(&through_tera, data), "{reference}");
        }
    }

    #[test]
    fn a_dotted_path_compiles_to_a_direct_lookup() {
        let (_, compiled) = compile("{{ a.b.c }}");

        assert!(matches!(compiled, CompiledTemplate::Simple(key) if key.as_str() == "a.b.c"));
    }

    #[test]
    fn the_whole_record_reference_compiles_to_a_direct_lookup() {
        let (_, compiled) = compile("{{ context }}");

        assert!(matches!(compiled, CompiledTemplate::Simple(key) if key.as_str() == "context"));
    }

    #[test]
    fn whitespace_around_a_field_reference_compiles_to_the_same_direct_lookup() {
        for source in ["{{   id   }}", "{{id}}", "{{\n\tid\n}}"] {
            let (_, compiled) = compile(source);

            assert!(
                matches!(&compiled, CompiledTemplate::Simple(key) if key.as_str() == "id"),
                "{source}: {compiled:?}"
            );
        }
    }

    #[test]
    fn a_positional_accessor_with_leading_zeros_names_the_column_tera_would_index() {
        let (_, compiled) = compile("{{ this[007] }}");

        assert!(matches!(compiled, CompiledTemplate::Simple(key) if key.as_str() == "7"));
    }

    #[test]
    fn literal_text_around_several_field_references_compiles_to_a_concatenation() {
        let (_, compiled) = compile("/{{ country }}/{{ this['City Name'] }}-{{ this[0] }}");

        let CompiledTemplate::Composite(parts) = compiled else {
            panic!("expected a concatenation, got {compiled:?}");
        };
        let rendered: Vec<String> = parts
            .iter()
            .map(|part| match part {
                TemplatePart::Static(literal) => format!("static:{literal}"),
                TemplatePart::Dynamic(key) => format!("field:{key}"),
            })
            .collect();
        assert_eq!(rendered, ["static:/", "field:country", "static:/", "field:City Name", "static:-", "field:0"]);
    }

    #[test]
    fn a_closing_brace_after_a_field_reference_stays_literal_text_as_tera_reads_it() {
        assert_eq!(resolve("{{ a }}}", &json!({"a": "x"})), json!("x}"));
    }

    #[test]
    fn an_unclosed_field_reference_is_a_compile_error() {
        for source in ["Station-{{ id", "{{ a", "{{ a }} and {{ b"] {
            assert_eq!(compile_error(source).template, TemplateSource::new(source), "{source}");
        }
    }

    #[test]
    fn an_unclosed_tag_or_comment_is_a_compile_error() {
        for source in ["{%if a%}{{ a }}", "{% for x in xs %}{{ x }}", "{# note {{ a }}", "{% if a %}"] {
            assert_eq!(compile_error(source).template, TemplateSource::new(source), "{source}");
        }
    }

    #[test]
    fn an_expression_tera_cannot_parse_is_a_compile_error() {
        for source in ["{{ a.0 }}", "{{ a b }}", "{{ not }}", "{{ a.true }}"] {
            assert_eq!(compile_error(source).template, TemplateSource::new(source), "{source}");
        }
    }

    #[test]
    fn a_failed_compilation_leaves_earlier_templates_renderable() {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new("{{ value | upper }}")).unwrap();
        assert!(runner.compile(&TemplateSource::new("{{ value | upper ")).is_err());

        assert_eq!(runner.resolver().resolve(&compiled, &json!({"value": "x"})).unwrap(), json!("X"));
    }
}
