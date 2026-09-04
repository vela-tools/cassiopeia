use derive_more::Display;
use serde_json::Value;

/// The kind of JSON Schema constraint shown for a node in a detail pane.
///
/// The `Display` label is the exact text rendered in the UI (`Enum`, `Format`, and so on), so it is
/// kept in title case rather than the internal kebab-case convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum ConstraintKind {
    /// An `enum` list of permitted values.
    Enum,
    /// A `format` annotation such as `date-time` or `uri`.
    Format,
    /// A numeric `minimum` bound.
    Min,
    /// A numeric `maximum` bound.
    Max,
    /// A `pattern` the string value must match.
    Pattern,
}

/// One constraint row shown for a schema node: its kind and the rendered value text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    /// Which constraint this row describes.
    pub kind: ConstraintKind,
    /// The constraint's value, already rendered to display text.
    pub value: String,
}

impl Constraint {
    /// Builds a constraint row from its kind and rendered value.
    #[must_use]
    pub const fn new(kind: ConstraintKind, value: String) -> Constraint {
        Constraint { kind, value }
    }
}

/// Collects the constraint rows a schema node declares: enum, format, numeric bounds, and pattern.
///
/// The rows are gathered in a fixed order so the detail pane renders them consistently.
#[must_use]
pub fn node_constraints(schema: &Value) -> Vec<Constraint> {
    let mut constraints = Vec::new();

    if let Some(values) = schema.get("enum").and_then(|value| value.as_array()) {
        let joined = values.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        constraints.push(Constraint::new(ConstraintKind::Enum, joined));
    }
    if let Some(format) = schema.get("format").and_then(|value| value.as_str()) {
        constraints.push(Constraint::new(ConstraintKind::Format, format.to_string()));
    }
    if let Some(minimum) = schema.get("minimum") {
        constraints.push(Constraint::new(ConstraintKind::Min, minimum.to_string()));
    }
    if let Some(maximum) = schema.get("maximum") {
        constraints.push(Constraint::new(ConstraintKind::Max, maximum.to_string()));
    }
    if let Some(pattern) = schema.get("pattern").and_then(|value| value.as_str()) {
        constraints.push(Constraint::new(ConstraintKind::Pattern, pattern.to_string()));
    }

    constraints
}

#[cfg(test)]
mod tests {
    use crate::constraint::{Constraint, ConstraintKind, node_constraints};
    use serde_json::json;

    #[test]
    fn constraint_kinds_render_as_their_title_case_labels() {
        assert_eq!(ConstraintKind::Enum.to_string(), "Enum");
        assert_eq!(ConstraintKind::Pattern.to_string(), "Pattern");
    }

    #[test]
    fn every_declared_constraint_is_collected_in_order() {
        let schema = json!({
            "enum": ["a", "b"],
            "format": "uri",
            "minimum": 0,
            "maximum": 10,
            "pattern": "^x",
        });

        let constraints = node_constraints(&schema);

        assert_eq!(
            constraints,
            vec![
                Constraint::new(ConstraintKind::Enum, "\"a\", \"b\"".to_string()),
                Constraint::new(ConstraintKind::Format, "uri".to_string()),
                Constraint::new(ConstraintKind::Min, "0".to_string()),
                Constraint::new(ConstraintKind::Max, "10".to_string()),
                Constraint::new(ConstraintKind::Pattern, "^x".to_string()),
            ]
        );
    }

    #[test]
    fn a_node_without_constraints_yields_an_empty_list() {
        assert!(node_constraints(&json!({"type": "string"})).is_empty());
    }
}
