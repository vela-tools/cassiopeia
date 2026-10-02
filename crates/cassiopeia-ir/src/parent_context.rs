use crate::relationship_key::RelationshipKey;
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
/// back to its parent (or a parent to its child) by URN and relationship key.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, PartialEq, Eq)]
#[getset(get = "pub")]
pub struct ParentContext {
    /// The linked URN together with its direction relative to this fragment.
    urn: ParentContextType,
    /// Where the parent entity records the link: the path to the relationship (one segment for a
    /// top-level relationship, several for a nested one, ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with
    /// 4.5.3), or the instance of a multi-attribute relationship that minted it (clause 4.5.5).
    key: RelationshipKey,
}

impl ParentContext {
    /// Builds a parent context from a directed URN link and the key the link is recorded under.
    #[must_use]
    pub const fn new(urn: ParentContextType, key: RelationshipKey) -> ParentContext {
        ParentContext { urn, key }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        instance_index::InstanceIndex,
        parent_context::{ParentContext, ParentContextType},
        relationship_key::RelationshipKey,
        relationship_path::RelationshipPath,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use urn_rs::Urn;

    #[test]
    fn accessors_expose_the_link_and_its_key() {
        let urn = "urn:ngsi-ld:Road:1".parse::<Urn>().expect("valid urn");
        let context = ParentContext::new(
            ParentContextType::Child(urn.clone()),
            RelationshipKey::Path(RelationshipPath::flat(NameBuf::new("refRoad").expect("valid"))),
        );
        assert_eq!(context.urn(), &ParentContextType::Child(urn));
        assert_eq!(context.key().to_string(), "refRoad");
    }

    #[test]
    fn an_instance_link_carries_the_instance_that_minted_it() {
        let urn = "urn:ngsi-ld:Airport:535".parse::<Urn>().expect("valid urn");
        let key = RelationshipKey::Instance {
            attribute: NameBuf::new("servesAirports").expect("valid"),
            index: InstanceIndex::from(1),
        };
        let context = ParentContext::new(ParentContextType::Child(urn), key.clone());
        assert_eq!(context.key(), &key);
    }
}
