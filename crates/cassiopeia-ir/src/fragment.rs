use crate::parent_context::ParentContext;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use getset::Getters;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use urn_rs::Urn;

/// A partial intermediate representation of an entity, produced during early transformation.
#[derive(Debug, Clone, Serialize, Deserialize, Getters)]
#[getset(get = "pub")]
pub struct Fragment {
    /// The source data used to derive this fragment.
    source_data: Value,
    /// The pre-calculated target URN.
    target_urn: Urn,
    /// The resolved scope of the entity.
    scope: Option<NgsiLdScope>,
    /// Relationship context for child entities.
    parent_context: Option<Vec<ParentContext>>,
}

/// The owned constituent parts of a [`Fragment`], produced by [`Fragment::into_parts`].
pub type FragmentParts = (Value, Urn, Option<NgsiLdScope>, Option<Vec<ParentContext>>);

impl Fragment {
    /// Builds a fragment from its source data, target URN, scope, and optional parent context.
    #[must_use]
    pub const fn new(source_data: Value, target_urn: Urn, scope: Option<NgsiLdScope>, parent_context: Option<Vec<ParentContext>>) -> Fragment {
        Fragment {
            source_data,
            target_urn,
            scope,
            parent_context,
        }
    }

    /// Deconstructs the fragment into its owned parts, consuming it.
    ///
    /// The resolver is the fragment's last owner: it hands the record and the scope straight to the
    /// entity store, so taking them by value here is what keeps the source record from being deep
    /// copied on its way in.
    #[must_use]
    pub fn into_parts(self) -> FragmentParts {
        (self.source_data, self.target_urn, self.scope, self.parent_context)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        fragment::Fragment,
        parent_context::{ParentContext, ParentContextType},
        relationship_path::RelationshipPath,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use serde_json::json;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse::<Urn>().expect("valid urn")
    }

    #[test]
    fn accessors_expose_the_constructor_inputs() {
        let context = ParentContext::new(
            ParentContextType::Child(urn("urn:ngsi-ld:Road:1")),
            RelationshipPath::flat(NameBuf::new("refRoad").expect("valid")),
        );
        let fragment = Fragment::new(json!({"t": 1}), urn("urn:ngsi-ld:Station:1"), None, Some(vec![context.clone()]));
        assert_eq!(fragment.source_data(), &json!({"t": 1}));
        assert_eq!(fragment.target_urn(), &urn("urn:ngsi-ld:Station:1"));
        assert!(fragment.scope().is_none());
        assert_eq!(fragment.parent_context().as_ref(), Some(&vec![context]));
    }
}
