use crate::template::{
    CompiledTemplate,
    TemplatePart,
    TemplateSource,
    contrib,
    field_path::FieldPath,
    filter,
    function,
    resolver::TemplateResolver,
    template_name::TemplateName,
};
use lazy_regex::regex;
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
    pub fn compile(&mut self, source: &TemplateSource) -> CompiledTemplate {
        let source = source.as_str();

        if !source.contains("{{") && !source.contains("{%") {
            return CompiledTemplate::Static(source.to_string());
        }

        if Self::needs_tera(source) {
            let name = TemplateName::for_source(source);
            let tera = Arc::make_mut(&mut self.tera);
            // Registering the same expression twice is not an error: the name is a digest of the
            // expression, so a repeat registration replaces an identical template.
            let _ = tera.add_raw_template(name.as_str(), source);

            return CompiledTemplate::Complex(name);
        }

        Self::split(source)
    }

    /// Decides whether an expression needs the full templating engine.
    ///
    /// Positional and bracketed `this[...]` accessors are stripped first: a named form such as
    /// `this['CO(GT)']` embeds parentheses that would otherwise read as a Tera function call, and a
    /// positional `this[0]` addresses a headerless column that direct lookup already resolves.
    fn needs_tera(source: &str) -> bool {
        let bare = regex!(r"this\[(?:'[^']*'|\d+)\]").replace_all(source, "");

        bare.contains('|') || bare.contains('(') || bare.contains(" if ")
    }

    /// Splits an interpolating expression into literal and field-reference parts.
    fn split(source: &str) -> CompiledTemplate {
        let reference = regex!(r"\{\{\s*(?:this\['([^']+)'\]|this\[(\d+)\]|([^}]+))\s*\}\}");
        let mut parts = Vec::new();
        let mut consumed = 0;

        for captures in reference.captures_iter(source) {
            let Some(whole) = captures.get(0) else {
                continue;
            };
            if whole.start() > consumed {
                parts.push(TemplatePart::Static(source[consumed..whole.start()].to_string()));
            }

            // Group 1 is the named `this['Key']` form, group 2 the positional `this[0]` form (its
            // digits are the column key), and group 3 the bare `key` form.
            let key = captures
                .get(1)
                .or_else(|| captures.get(2))
                .or_else(|| captures.get(3))
                .map(|key| key.as_str().trim())
                .unwrap_or_default();
            parts.push(TemplatePart::Dynamic(FieldPath::new(key)));

            consumed = whole.end();
        }

        if consumed < source.len() {
            parts.push(TemplatePart::Static(source[consumed..].to_string()));
        }

        match parts.as_slice() {
            [TemplatePart::Dynamic(key)] => CompiledTemplate::Simple(key.clone()),
            _ => CompiledTemplate::Composite(parts),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::template::{CompiledTemplate, TemplateSource, runner::TemplateRunner};
    use serde_json::{Value as JsonValue, json};

    fn compile(source: &str) -> (TemplateRunner, CompiledTemplate) {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new(source));

        (runner, compiled)
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
    fn a_filtered_expression_compiles_to_a_tera_template() {
        let (_, compiled) = compile("{{ name | upper }}");

        assert!(matches!(compiled, CompiledTemplate::Complex(_)));
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
        let first = runner.compile(&TemplateSource::new("{{ name | upper }}"));
        let second = runner.compile(&TemplateSource::new("{{ name | upper }}"));

        match (first, second) {
            (CompiledTemplate::Complex(first), CompiledTemplate::Complex(second)) => assert_eq!(first, second),
            other => panic!("expected two complex templates, got {other:?}"),
        }
    }

    #[test]
    fn a_resolver_taken_after_compilation_can_render_the_registered_template() {
        let mut runner = TemplateRunner::new();
        let compiled = runner.compile(&TemplateSource::new("{{ value | upper }}"));
        let resolver = runner.resolver();

        assert_eq!(resolver.resolve(&compiled, &json!({"value": "x"})).unwrap(), json!("X"));
    }

    #[test]
    fn rendering_an_expression_over_a_missing_field_is_an_error() {
        let (runner, compiled) = compile("{{ missing | upper }}");

        assert!(runner.resolver().resolve(&compiled, &json!({})).is_err());
    }
}
