use crate::parent_context::ParentContext;
use cassiopeia_mapping::template::run_variables::RunVariables;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use chrono::{DateTime, Utc};
use getset::Getters;
use serde_json::Value;
use typed_builder::TypedBuilder;
use urn_rs::Urn;

/// A partial intermediate representation of an entity, produced during early transformation.
#[derive(Debug, Clone, Getters, TypedBuilder)]
#[getset(get = "pub")]
pub struct Fragment {
    /// The source data used to derive this fragment.
    source_data: Value,
    /// The pre-calculated target URN.
    target_urn: Urn,
    /// The resolved scope of the entity.
    #[builder(default = None)]
    scope: Option<NgsiLdScope>,
    /// The resolved entity `expiresAt` (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
    #[builder(default = None)]
    expires_at: Option<DateTime<Utc>>,
    /// Relationship context for child entities.
    #[builder(default = None)]
    parent_context: Option<Vec<ParentContext>>,
    /// The run variables of the input the source data came from, which every template of the
    /// fragment's mapping reads it with; none for an input that sets no variables.
    #[builder(default = RunVariables::none())]
    variables: RunVariables,
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
    /// The run variables the source data is read with.
    pub variables: RunVariables,
}

impl Fragment {
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
            variables: self.variables,
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
    use cassiopeia_mapping::template::run_variables::RunVariables;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, json};
    use urn_rs::Urn;

    fn urn(value: &str) -> Urn {
        value.parse::<Urn>().expect("valid urn")
    }

    #[test]
    fn accessors_expose_the_builder_inputs() {
        let context = ParentContext::new(
            ParentContextType::Child(urn("urn:ngsi-ld:Road:1")),
            RelationshipKey::Path(RelationshipPath::flat(NameBuf::new("refRoad").expect("valid"))),
        );
        let expires_at = Utc.with_ymd_and_hms(2026, 10, 25, 13, 0, 0).unwrap();
        let fragment = Fragment::builder()
            .source_data(json!({"t": 1}))
            .target_urn(urn("urn:ngsi-ld:Station:1"))
            .expires_at(Some(expires_at))
            .parent_context(Some(vec![context.clone()]))
            .build();
        assert_eq!(fragment.source_data(), &json!({"t": 1}));
        assert_eq!(fragment.target_urn(), &urn("urn:ngsi-ld:Station:1"));
        assert_eq!(fragment.scope(), &None);
        assert_eq!(fragment.expires_at(), &Some(expires_at));
        assert_eq!(fragment.parent_context().as_ref(), Some(&vec![context]));
        assert_eq!(fragment.variables(), &RunVariables::none());
    }

    #[test]
    fn a_fragment_carries_the_run_variables_of_its_input_into_its_parts() {
        let mut variables = Map::new();
        variables.insert("network".to_string(), json!("bikes"));
        let variables = RunVariables::new(variables);

        let parts = Fragment::builder()
            .source_data(json!({"id": 1}))
            .target_urn(urn("urn:ngsi-ld:Station:bikes-1"))
            // The fragment takes its own handle on the shared map, while the test keeps one to compare.
            .variables(variables.clone())
            .build()
            .into_parts();

        assert_eq!(parts.source_data, json!({"id": 1}));
        assert_eq!(parts.variables.address(), variables.address());
    }
}
