pub mod compile_error;
pub mod contrib;
pub(crate) mod direct_form;
pub mod error;
pub mod field_path;
pub mod filter;
pub mod function;
pub mod numeric_value;
pub mod resolver;
pub mod runner;
pub mod template_name;
pub mod value_expression;

use crate::template::{field_path::FieldPath, template_name::TemplateName};
use derive_more::Display;
use serde::{Deserialize, Serialize};

/// A raw, uncompiled template expression exactly as it was written in a mapping document.
///
/// Kept distinct from an evaluated result so that a template that has not been through
/// `TemplateRunner::compile` cannot be mistaken for a value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(transparent)]
pub struct TemplateSource(String);

impl TemplateSource {
    /// Wraps a raw template expression.
    #[must_use]
    pub fn new(source: impl Into<String>) -> TemplateSource {
        TemplateSource(source.into())
    }

    /// The expression as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The evaluation strategy chosen for one template expression, decided once at mapping load time.
///
/// Most mapping expressions are a bare field reference or a short concatenation. Recognising those
/// shapes up front lets the hot loop resolve them without entering Tera at all.
#[derive(Clone, Debug)]
pub enum CompiledTemplate {
    /// A literal with no interpolation, such as `Start`.
    Static(String),

    /// A single field reference, such as `{{ field }}`, resolved by direct lookup.
    Simple(FieldPath),

    /// A concatenation of literals and field references, such as `Station-{{ id }}`.
    Composite(Vec<TemplatePart>),

    /// A template whose whole output is one expression needing filters, arithmetic, or a guard,
    /// such as `{{ field | split(pat=" ") }}` or `{% if field %}{{ field | upper }}{% endif %}`.
    ///
    /// It resolves to the expression's value with its type intact, so an array or object a filter
    /// returns reaches the caller as that array or object rather than as its printed form; a guard
    /// that suppresses the expression resolves to null. Holds the name the template is registered
    /// under in the Tera engine.
    Expression(TemplateName),

    /// A template that renders text around or between its expressions, such as
    /// `{% if field %}{{ field }}{% else %}none{% endif %}`, and so always resolves to a string.
    /// Holds the name the template is registered under in the Tera engine.
    Complex(TemplateName),
}

/// One segment of a `CompiledTemplate::Composite`.
#[derive(Clone, Debug)]
pub enum TemplatePart {
    /// Literal text, emitted as-is.
    Static(String),

    /// A source field path, looked up per record.
    Dynamic(FieldPath),
}

#[cfg(test)]
mod tests {
    use crate::template::TemplateSource;

    #[test]
    fn a_template_source_round_trips_through_json_transparently() {
        let source: TemplateSource = serde_json::from_str(r#""{{ id }}""#).unwrap();

        assert_eq!(source.as_str(), "{{ id }}");
        assert_eq!(serde_json::to_string(&source).unwrap(), r#""{{ id }}""#);
    }
}
