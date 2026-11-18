use cassiopeia_ir::source_position::SourcePosition;
use urn_rs::Urn;

/// One parent-child link a fragment records: the parent links to the child under the relationship
/// key `property`.
///
/// The key is the text form of a
/// [`RelationshipKey`](cassiopeia_ir::relationship_key::RelationshipKey): a relationship path, or an
/// attribute name and an instance index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RelationshipEdge {
    /// The entity that holds the relationship.
    pub parent: Urn,
    /// The relationship key the parent records the child under.
    pub property: String,
    /// The entity the relationship targets.
    pub child: Urn,
}

/// The edges one fragment records, together with where the fragment stands in the run's sources.
///
/// A relationship store keeps the position beside every edge, so the targets that several records
/// give one parent are read back in the order of the source whatever order the writes arrive in.
/// The edges of one fragment share its position and are listed in the order its record names their
/// targets.
#[derive(Debug, Clone, Copy)]
pub struct FragmentEdges<'edges> {
    /// Where the fragment that records the edges stands in the run's sources.
    pub position: SourcePosition,
    /// The edges, in the order the fragment's record names their targets.
    pub edges: &'edges [RelationshipEdge],
}

#[cfg(test)]
mod tests {
    use crate::relationship_store::relationship_edge::{FragmentEdges, RelationshipEdge};
    use cassiopeia_ir::source_position::SourcePosition;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_fragment_s_edges_carry_its_position_and_keep_their_order() {
        let edge = |child: &str| RelationshipEdge {
            parent: "urn:ngsi-ld:Stop:S1".parse().unwrap(),
            property: "servedBy".to_owned(),
            child: child.parse().unwrap(),
        };
        let edges = [edge("urn:ngsi-ld:Line:U2"), edge("urn:ngsi-ld:Line:U1")];
        let position = SourcePosition::first_of(1).advanced(4);

        let fragment = FragmentEdges { position, edges: &edges };

        assert_eq!(fragment.position, position);
        assert_eq!(
            fragment.edges.iter().map(|edge| edge.child.as_str()).collect::<Vec<&str>>(),
            ["urn:ngsi-ld:Line:U2", "urn:ngsi-ld:Line:U1"]
        );
    }
}
