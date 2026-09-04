use crate::constraint::Constraint;

/// The human-readable facts shown in a detail pane for one schema node.
///
/// Shared by the mapping wizard and the read-only explorer. The node's own key is not stored here:
/// callers already hold the path a detail record is keyed by, so the field name is the path tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaNodeDetails {
    /// The node's cleaned description text.
    pub description: String,
    /// The node's display type label.
    pub data_type: String,
    /// Whether the node is marked required by its parent schema.
    pub required: bool,
    /// The constraint rows declared on the node.
    pub constraints: Vec<Constraint>,
}

#[cfg(test)]
mod tests {
    use crate::{
        constraint::{Constraint, ConstraintKind},
        schema_node_details::SchemaNodeDetails,
    };

    #[test]
    fn a_detail_record_holds_its_facts() {
        let details = SchemaNodeDetails {
            description: "A field.".to_string(),
            data_type: "string".to_string(),
            required: true,
            constraints: vec![Constraint::new(ConstraintKind::Format, "uri".to_string())],
        };

        assert!(details.required);
        assert_eq!(details.constraints.len(), 1);
    }
}
