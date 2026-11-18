use cassiopeia_diagnostic::code::{expander_code::ExpanderCode, extractor_code::ExtractorCode, geometry_code::GeometryCode, transform_code::TransformCode};
use cassiopeia_expander::{
    error::ExpanderError,
    urn::{error::UrnError, scope_error::ScopeError},
};
use cassiopeia_extractor::error::ExtractionError;
use cassiopeia_geometry::error::GeometryError;
use cassiopeia_transformer::error::TransformationError;

/// A stage failure that names the diagnostic code it is published under.
///
/// One capability, declared here rather than on each error type, because the error types belong to
/// their own crates and the code vocabulary belongs to the diagnostic crate: this composition root
/// is the only place that knows both. Every implementation matches exhaustively, so adding a variant
/// to a stage's error enum is a compile error rather than a silently new published code.
pub(crate) trait CodedError {
    /// The subsystem code enum this error's reasons come from.
    type Code;

    /// Which reason this failure is.
    fn code(&self) -> Self::Code;
}

impl CodedError for ExpanderError {
    type Code = ExpanderCode;

    fn code(&self) -> ExpanderCode {
        match self {
            ExpanderError::Urn(UrnError::GeneratedIdEmpty { .. }) => ExpanderCode::IdAbsent,
            ExpanderError::Urn(UrnError::Template(_) | UrnError::BuildUrn { .. } | UrnError::RelationshipMissingSource | UrnError::NoRelationshipTarget) => {
                ExpanderCode::IdUnresolvable
            }
            ExpanderError::Scope(ScopeError::Template(_)) => ExpanderCode::ScopeUnresolvable,
            ExpanderError::Scope(
                ScopeError::Invalid { .. } | ScopeError::FieldNotText { .. } | ScopeError::ValueNotText { .. } | ScopeError::ItemNotText { .. },
            ) => ExpanderCode::ScopeUnreadable,
            ExpanderError::Expiry(_) => ExpanderCode::ExpiresAtUnresolvable,
            ExpanderError::UnmatchedCollection(_) => ExpanderCode::CollectionUnmatched,
            ExpanderError::CollectionMissing => ExpanderCode::CollectionMissing,
        }
    }
}

impl CodedError for ExtractionError {
    type Code = ExtractorCode;

    fn code(&self) -> ExtractorCode {
        match self {
            ExtractionError::RecursionLimitExceeded { .. } => ExtractorCode::RecursionLimitExceeded,
        }
    }
}

impl CodedError for TransformationError {
    type Code = TransformCode;

    fn code(&self) -> TransformCode {
        match self {
            TransformationError::Build { .. } => TransformCode::EntityUnbuildable,
        }
    }
}

impl CodedError for GeometryError {
    type Code = GeometryCode;

    fn code(&self) -> GeometryCode {
        match self {
            GeometryError::GeometryCollection => GeometryCode::CollectionInadmissible,
            GeometryError::Uncoercible { .. } => GeometryCode::Uncoercible,
            GeometryError::AmbiguousMultiGeometry { .. } => GeometryCode::AmbiguousMultiGeometry,
            GeometryError::StrategyNotApplicable { .. } => GeometryCode::StrategyNotApplicable,
            GeometryError::Unbuildable { .. } => GeometryCode::Unbuildable,
            GeometryError::ShortPosition { .. } => GeometryCode::ShortPosition,
            GeometryError::ShortLineString { .. } => GeometryCode::ShortLineString,
            GeometryError::ShortRing { .. } => GeometryCode::ShortRing,
            GeometryError::UnclosedRing => GeometryCode::UnclosedRing,
            GeometryError::EmptyGeometry => GeometryCode::EmptyGeometry,
            GeometryError::MixedCollection => GeometryCode::MixedCollection,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::stages::coded_error::CodedError;
    use cassiopeia_common::collection::CollectionName;
    use cassiopeia_diagnostic::code::{expander_code::ExpanderCode, geometry_code::GeometryCode, transform_code::TransformCode};
    use cassiopeia_expander::{
        compiler::ExpanderCompiler,
        error::ExpanderError,
        expander::Expander,
        expansion_losses::ExpansionLosses,
        generic::GenericExpander,
        router::MappingRouter,
    };
    use cassiopeia_geometry::{error::GeometryError, geometry::GeometryKind};
    use cassiopeia_ir::{record::Record, source_position::SourcePosition};
    use cassiopeia_mapping::{
        mapping::Mapping,
        template::{run_variables::RunVariables, runner::TemplateRunner},
    };
    use cassiopeia_ngsi_ld::entity::error::{MandatoryMember, NgsiLdError};
    use cassiopeia_transformer::error::TransformationError;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{path::Path, sync::Arc};

    /// The code the expander stage publishes when expanding `data` under the mapping `document` fails.
    fn expansion_code(document: &str, data: Value) -> ExpanderCode {
        let mut runner = TemplateRunner::new();
        let mut mapping = Mapping::from_json5(document, Path::new("sites.json5"), &mut runner).unwrap();
        ExpanderCompiler::compile(&mut mapping, Path::new("sites.json5"), &mut runner).unwrap();
        let expander = GenericExpander::new(MappingRouter::Single(Arc::new(mapping)), runner.resolver(), RunVariables::none());
        let Value::Object(data) = data else {
            panic!("test record must be a JSON object");
        };

        let Err(error) = Expander::expand(&expander, Record::new(None, data), SourcePosition::first_of(0), &ExpansionLosses::new()) else {
            panic!("the record expanded");
        };
        error.code()
    }

    /// A `Site` mapping named by `entity_name`, declaring `scope` as its scope when one is given.
    fn site_mapping(entity_name: &str, scope: Option<&str>) -> String {
        let scope = scope.map_or_else(String::new, |scope| format!("scope: {scope},"));
        r#"{
            version: "v4",
            dataModel: "Site",
            identity: { entityName: "ENTITY_NAME" },
            SCOPE
            attributes: { a: { source: "{{ a }}" } },
        }"#
        .replace("ENTITY_NAME", entity_name)
        .replace("SCOPE", &scope)
    }

    /// The code a `Site` record named by `{{ site }}` is skipped under for its `scope`.
    fn scope_code(scope: &str, data: Value) -> ExpanderCode {
        expansion_code(&site_mapping("{{ site }}", Some(scope)), data)
    }

    /// A `Site` mapping whose `owner` Relationship declares its target and the members `owner`.
    fn owner_mapping(owner: &str) -> String {
        r#"{
            version: "v4",
            dataModel: "Site",
            identity: { entityName: "{{ site }}" },
            attributes: { owner: { type: "Relationship", target: { entity: "Organization" }, OWNER } },
        }"#
        .replace("OWNER", owner)
    }

    #[test]
    fn an_identity_over_a_missing_field_is_absent() {
        assert_eq!(expansion_code(&site_mapping("{{ site }}", None), json!({"a": 1})), ExpanderCode::IdAbsent);
    }

    #[test]
    fn an_identity_cleaning_to_nothing_is_absent() {
        assert_eq!(
            expansion_code(&site_mapping("{{ site }}", None), json!({"site": "-", "a": 1})),
            ExpanderCode::IdAbsent
        );
    }

    #[test]
    fn an_identity_template_that_fails_to_render_is_unresolvable() {
        let document = site_mapping("{{ site | split(pat='-') | first | upper }}", None);

        assert_eq!(expansion_code(&document, json!({"site": 5, "a": 1})), ExpanderCode::IdUnresolvable);
    }

    #[test]
    fn an_identity_under_an_entity_type_that_cannot_form_a_urn_is_unresolvable() {
        let document = site_mapping("{{ site }}", None).replace(r#"dataModel: "Site""#, r#"dataModel: "Straße""#);

        assert_eq!(expansion_code(&document, json!({"site": "A", "a": 1})), ExpanderCode::IdUnresolvable);
    }

    #[test]
    fn a_relationship_with_a_target_and_no_source_is_unresolvable() {
        assert_eq!(expansion_code(&owner_mapping(""), json!({"site": "A"})), ExpanderCode::IdUnresolvable);
    }

    #[test]
    fn a_relationship_instance_with_no_source_is_unresolvable() {
        let owner = r#"instances: [
            { source: "{{ owner }}" },
            { properties: { datasetId: { source: "urn:ngsi-ld:dataset:role:former" } } },
        ]"#;

        assert_eq!(
            expansion_code(&owner_mapping(owner), json!({"site": "A", "owner": "O1"})),
            ExpanderCode::IdUnresolvable
        );
    }

    #[test]
    fn a_scope_template_that_fails_to_render_is_unresolvable() {
        assert_eq!(scope_code(r#""{{ zone | upper }}""#, json!({"site": "A"})), ExpanderCode::ScopeUnresolvable);
    }

    #[test]
    fn a_scope_that_is_no_legal_scope_is_unreadable() {
        assert_eq!(
            scope_code(r#""/{{ zone }}""#, json!({"site": "A", "zone": "Novo mesto"})),
            ExpanderCode::ScopeUnreadable
        );
    }

    #[test]
    fn a_scope_reading_a_boolean_is_unreadable() {
        assert_eq!(
            scope_code(r#""{{ indoor }}""#, json!({"site": "A", "indoor": true})),
            ExpanderCode::ScopeUnreadable
        );
    }

    #[test]
    fn a_scope_resolving_to_the_ngsi_ld_null_is_unreadable() {
        assert_eq!(
            scope_code(r#""{{ s }}""#, json!({"site": "A", "s": "urn:ngsi-ld:null"})),
            ExpanderCode::ScopeUnreadable
        );
    }

    #[test]
    fn a_scope_list_with_an_item_that_is_not_text_is_unreadable() {
        assert_eq!(
            scope_code(r#""{{ areas }}""#, json!({"site": "A", "areas": ["/A", 5]})),
            ExpanderCode::ScopeUnreadable
        );
    }

    #[test]
    fn an_expander_failure_names_its_reason() {
        assert_eq!(ExpanderError::CollectionMissing.code(), ExpanderCode::CollectionMissing);
        assert_eq!(
            ExpanderError::UnmatchedCollection(CollectionName::from("Camera")).code(),
            ExpanderCode::CollectionUnmatched
        );
    }

    #[test]
    fn a_transform_failure_names_its_reason() {
        let error = TransformationError::Build {
            source: NgsiLdError::MissingMandatoryField {
                member: MandatoryMember::ObjectList,
            },
        };

        assert_eq!(error.code(), TransformCode::EntityUnbuildable);
    }

    #[test]
    fn a_geometry_refusal_names_its_reason() {
        assert_eq!(GeometryError::GeometryCollection.code(), GeometryCode::CollectionInadmissible);
        assert_eq!(
            GeometryError::AmbiguousMultiGeometry {
                origin: GeometryKind::MultiPolygon,
                members: 2,
            }
            .code(),
            GeometryCode::AmbiguousMultiGeometry
        );
    }
}
