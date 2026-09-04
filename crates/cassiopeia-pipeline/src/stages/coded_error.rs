use cassiopeia_diagnostic::code::{expander_code::ExpanderCode, extractor_code::ExtractorCode, geometry_code::GeometryCode, transform_code::TransformCode};
use cassiopeia_expander::error::ExpanderError;
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
            ExpanderError::Urn(_) => ExpanderCode::UrnUngeneratable,
            ExpanderError::UnmatchedCollection(_) => ExpanderCode::CollectionUnmatched,
            ExpanderError::CollectionMissing => ExpanderCode::CollectionMissing,
        }
    }
}

impl CodedError for ExtractionError {
    type Code = ExtractorCode;

    fn code(&self) -> ExtractorCode {
        match self {
            ExtractionError::Template { .. } => ExtractorCode::TemplateUnresolvable,
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
    use cassiopeia_expander::error::ExpanderError;
    use cassiopeia_geometry::{error::GeometryError, geometry::GeometryKind};
    use cassiopeia_ngsi_ld::entity::error::{MandatoryMember, NgsiLdError};
    use cassiopeia_transformer::error::TransformationError;

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
