use cassiopeia_mapping::{
    attribute::{Attribute, instance::AttributeInstance},
    mapping::Mapping,
    scope::{CompiledScope, Scope},
    template::{CompiledTemplate, TemplateSource, runner::TemplateRunner},
};
use serde_json::Value;

/// Compiles a mapping's raw template expressions into their fast, pre-classified form.
///
/// A loaded [`Mapping`] carries its templates as raw [`TemplateSource`] strings. Before the hot
/// expansion loop runs, every one of them (the identity name, the scope, and each attribute's
/// source, recursively through nested mappings, language maps, properties, and synthetic entities)
/// is compiled once and cached back onto the mapping so per-record expansion never re-parses a
/// template.
///
/// Compilation must finish before a [`TemplateResolver`](cassiopeia_mapping::template::resolver::TemplateResolver)
/// is taken from the runner: a resolver only sees the templates registered before it was handed out.
pub struct ExpanderCompiler;

impl ExpanderCompiler {
    /// Compiles every template in `mapping` in place.
    pub fn compile(mapping: &mut Mapping, runner: &mut TemplateRunner) {
        let compiled_name = runner.compile(mapping.identity().entity_name());
        mapping.identity_mut().set_compiled_entity_name(Some(compiled_name));

        if let Some(scope) = mapping.identity().scope() {
            let compiled_scope = match scope {
                Scope::Single(source) => CompiledScope::Single(runner.compile(source)),
                Scope::Multiple(sources) => CompiledScope::Multiple(sources.iter().map(|source| runner.compile(source)).collect()),
            };
            mapping.identity_mut().set_compiled_scope(Some(compiled_scope));
        }

        for attribute in mapping.attributes_mut().values_mut() {
            Self::compile_attribute(attribute, runner);
        }
    }

    /// Compiles one attribute's source and recurses into everything nested beneath it.
    fn compile_attribute(attribute: &mut Attribute, runner: &mut TemplateRunner) {
        Self::compile_source(attribute, runner);

        for nested in attribute.mappings_mut().values_mut() {
            Self::compile_attribute(nested, runner);
        }
        for language in attribute.language_map_mut().values_mut() {
            Self::compile_attribute(language, runner);
        }
        if let Some(properties) = attribute.properties_mut() {
            for property in properties.values_mut() {
                Self::compile_attribute(property, runner);
            }
        }
        if let Some(instances) = attribute.instances_mut() {
            for instance in instances {
                Self::compile_instance(instance, runner);
            }
        }
        if let Some(synthetic) = attribute.synthetic_entity_mut() {
            Self::compile(synthetic, runner);
        }
    }

    /// Compiles one multi-instance instance's source and its own properties.
    fn compile_instance(instance: &mut AttributeInstance, runner: &mut TemplateRunner) {
        instance.set_compiled_source(Self::compile_source_templates(instance.source().as_ref(), runner));

        if let Some(properties) = instance.properties_mut() {
            for property in properties.values_mut() {
                Self::compile_attribute(property, runner);
            }
        }
    }

    /// Compiles an attribute's `source`, which may be a single template or a list of them.
    fn compile_source(attribute: &mut Attribute, runner: &mut TemplateRunner) {
        let compiled = Self::compile_source_templates(attribute.source().as_ref(), runner);
        attribute.set_compiled_source(compiled);
    }

    /// Compiles a `source` value into its templates: a single string, a list of them, or a literal
    /// carried verbatim. Returns `None` when there is no source to compile.
    fn compile_source_templates(source: Option<&Value>, runner: &mut TemplateRunner) -> Option<Vec<CompiledTemplate>> {
        let mut compiled = Vec::new();

        if let Some(source) = source {
            match source {
                Value::String(text) => compiled.push(runner.compile(&TemplateSource::new(text))),
                Value::Array(items) => {
                    for item in items {
                        match item {
                            Value::String(text) => compiled.push(runner.compile(&TemplateSource::new(text))),
                            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
                                compiled.push(CompiledTemplate::Static(item.to_string()));
                            }
                        }
                    }
                }
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {
                    compiled.push(CompiledTemplate::Static(source.to_string()));
                }
            }
        }

        if compiled.is_empty() { None } else { Some(compiled) }
    }
}
