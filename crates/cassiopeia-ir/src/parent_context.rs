use crate::relationship_path::RelationshipPath;
use getset::Getters;
use serde::{Deserialize, Serialize};
use urn_rs::Urn;

/// The direction of a parent/child relationship link between entities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ParentContextType {
    /// The linked URN is the parent of the fragment carrying this context.
    Parent(Urn),
    /// The linked URN is a child of the fragment carrying this context.
    Child(Urn),
}

/// Relationship context attached to a fragment so a child entity can be linked
/// back to its parent (or a parent to its child) by URN and relationship path.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, PartialEq, Eq)]
#[getset(get = "pub")]
pub struct ParentContext {
    /// The linked URN together with its direction relative to this fragment.
    urn: ParentContextType,
    /// The path from the parent entity to the relationship the link is recorded under: a one-segment
    /// path for a top-level relationship, a multi-segment path for a nested one (ETSI GS CIM 009
    /// v1.9.1 clause 4.5.2.2 with 4.5.3).
    property: RelationshipPath,
}

impl ParentContext {
    /// Builds a parent context from a directed URN link and its relationship path.
    #[must_use]
    pub const fn new(urn: ParentContextType, property: RelationshipPath) -> ParentContext {
        ParentContext { urn, property }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        parent_context::{ParentContext, ParentContextType},
        relationship_path::RelationshipPath,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use urn_rs::Urn;

    #[test]
    fn accessors_expose_the_link_and_property() {
        let urn = "urn:ngsi-ld:Road:1".parse::<Urn>().expect("valid urn");
        let context = ParentContext::new(
            ParentContextType::Child(urn.clone()),
            RelationshipPath::flat(NameBuf::new("refRoad").expect("valid")),
        );
        assert_eq!(context.urn(), &ParentContextType::Child(urn));
        assert_eq!(context.property().to_string(), "refRoad");
    }
}
