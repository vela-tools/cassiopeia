use cassiopeia_mapping::template::error::TemplateError;
use std::result;
use thiserror::Error;

/// Why one attribute declaration could not be resolved against its source record.
///
/// A template failure costs only the attribute it belongs to, so the extractor records it and carries
/// on; a runaway recursion is a fault of the mapping as a whole and fails the entity.
#[derive(Debug, Error)]
pub(crate) enum ResolutionError {
    /// Evaluating the attribute's template against the source record failed.
    #[error("An attribute's template could not be resolved")]
    Template {
        #[from]
        source: TemplateError,
    },

    /// The attribute's nested declarations recursed past the guard depth, which points to a
    /// self-referential mapping rather than legitimately deep data.
    #[error("Attribute recursion limit exceeded at depth {depth}")]
    RecursionLimitExceeded {
        /// The depth at which the limit tripped.
        depth: usize,
    },
}

/// The result type used while resolving one attribute.
pub(crate) type Result<T> = result::Result<T, ResolutionError>;

#[cfg(test)]
mod tests {
    use crate::attribute::resolution_error::ResolutionError;
    use cassiopeia_mapping::{
        template::{
            TemplateSource,
            error::{ResolutionFailure, TemplateError},
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use std::{path::Path, sync::Arc};

    #[test]
    fn a_template_failure_converts_into_the_template_variant() {
        let failure = TemplateError {
            location: TemplateLocation::new(Arc::from(Path::new("sensor.json5")), TemplateSite::Attribute(NameBuf::new("codes").unwrap())),
            failure: Box::new(ResolutionFailure::Decode {
                template: TemplateSource::new("{{ a | json_decode }}"),
                source: serde_json::from_str::<serde_json::Value>("[").unwrap_err(),
            }),
        };

        let error = ResolutionError::from(failure);

        assert!(matches!(error, ResolutionError::Template { .. }));
    }
}
