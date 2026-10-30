use crate::parent_context::ParentContext;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use chrono::{DateTime, Utc};
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
    /// The resolved entity `expiresAt` (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
    expires_at: Option<DateTime<Utc>>,
    /// Relationship context for child entities.
    parent_context: Option<Vec<ParentContext>>,
}

/// The owned constituent parts of a [`Fragment`], produced by [`Fragment::into_parts`].
///
/// Each field is the [`Fragment`] field of the same name.
#[derive(Debug)]
pub struct FragmentParts {
    /// The source data used to derive the fragment.
    pub source_data: Value,
    /// The pre-calculated target URN.
    pub target_urn: Urn,
    /// The resolved scope of the entity.
    pub scope: Option<NgsiLdScope>,
    /// The resolved entity `expiresAt` (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
    pub expires_at: Option<DateTime<Utc>>,
    /// Relationship context for child entities.
    pub parent_context: Option<Vec<ParentContext>>,
}

impl Fragment {
    /// Builds a fragment from its source data, target URN, scope, entity `expiresAt`, and optional
    /// parent context.
    #[must_use]
    pub const fn new(
        source_data: Value,
        target_urn: Urn,
        scope: Option<NgsiLdScope>,
        expires_at: Option<DateTime<Utc>>,
        parent_context: Option<Vec<ParentContext>>,
    ) -> Fragment {
        Fragment {
            source_data,
            target_urn,
            scope,
            expires_at,
            parent_context,
        }
    }

    /// Deconstructs the fragment into its owned parts, consuming it.
    ///
    /// The resolver is the fragment's last owner: it hands the record, the scope, and the expiry
    /// straight to the entity store, so taking them by value here is what keeps the source record
    /// from being deep copied on its way in.
    #[must_use]
    pub fn into_parts(self) -> FragmentParts {
        FragmentParts {
            source_data: self.source_data,
            target_urn: self.target_urn,
            scope: self.scope,
            expires_at: self.expires_at,
            parent_context: self.parent_context,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        fragment::Fragment,
        parent_context::{ParentContext, ParentContextType},
        relationship_key::RelationshipKey,
        relationship_path::RelationshipPath,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse::<Urn>().expect("valid urn")
    }

    #[test]
    fn accessors_expose_the_constructor_inputs() {
        let context = ParentContext::new(
            ParentContextType::Child(urn("urn:ngsi-ld:Road:1")),
            RelationshipKey::Path(RelationshipPath::flat(NameBuf::new("refRoad").expect("valid"))),
        );
        let expires_at = Utc.with_ymd_and_hms(2026, 10, 25, 13, 0, 0).unwrap();
        let fragment = Fragment::new(
            json!({"t": 1}),
            urn("urn:ngsi-ld:Station:1"),
            None,
            Some(expires_at),
            Some(vec![context.clone()]),
        );
        assert_eq!(fragment.source_data(), &json!({"t": 1}));
        assert_eq!(fragment.target_urn(), &urn("urn:ngsi-ld:Station:1"));
        assert_eq!(fragment.scope(), &None);
        assert_eq!(fragment.expires_at(), &Some(expires_at));
        assert_eq!(fragment.parent_context().as_ref(), Some(&vec![context]));
    }
}
