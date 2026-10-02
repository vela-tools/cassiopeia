use crate::{
    template::{TemplateSource, engine_report::EngineReport, field_path::FieldPath},
    template_location::TemplateLocation,
};
use std::result;
use thiserror::Error;

/// A compiled template that failed to resolve against one source record.
///
/// It renders as the headline of the failure, naming the template's declaration and mapping
/// document; the [`ResolutionFailure`] it chains names the template as written and says what to
/// change, and chains Tera's own report in turn.
#[derive(Debug, Error)]
#[error("The {location} could not be resolved")]
pub struct TemplateError {
    /// Where the failing template is declared.
    pub location: TemplateLocation,
    /// Why it failed, boxed so a `Result` carrying the error stays small on the per-record path.
    #[source]
    pub failure: Box<ResolutionFailure>,
}

/// Why a compiled template failed to resolve against one source record.
#[derive(Debug, Error)]
pub enum ResolutionFailure {
    /// Tera failed on a field the record does not have.
    #[error(
        "`{template}` reads `{field}`, which this record does not have: guard it with `{{% if {field} %}}…{{% endif %}}` or default it with `{field} | default(value=…)`"
    )]
    MissingField {
        /// The template exactly as the mapping document wrote it.
        template: TemplateSource,
        /// The field the template reads that the record lacks.
        field: FieldPath,
        /// Tera's report.
        #[source]
        source: EngineReport,
    },

    /// Tera failed on a field that is null in the record.
    #[error(
        "`{template}` reads `{field}`, which is null in this record: guard it with `{{% if {field} %}}…{{% endif %}}` or default it with `{field} | default(value=…, boolean=true)`"
    )]
    NullField {
        /// The template exactly as the mapping document wrote it.
        template: TemplateSource,
        /// The field the template reads that is null in the record.
        field: FieldPath,
        /// Tera's report.
        #[source]
        source: EngineReport,
    },

    /// Tera failed to render the template for any other reason.
    #[error("`{template}` failed to render")]
    Render {
        /// The template exactly as the mapping document wrote it.
        template: TemplateSource,
        /// Tera's report.
        #[source]
        source: EngineReport,
    },

    /// A template whose output is one expression rendered text that is not the JSON encoding of a
    /// value, which means the encoding filter it was registered with did not produce its output.
    #[error("`{template}` did not render its value as JSON")]
    Decode {
        /// The template exactly as the mapping document wrote it.
        template: TemplateSource,
        /// The parse failure of the rendered text.
        #[source]
        source: serde_json::Error,
    },
}

/// The result type used throughout template evaluation.
pub type Result<T, E = TemplateError> = result::Result<T, E>;

#[cfg(test)]
mod tests {
    use crate::{
        template::{
            TemplateSource,
            engine_report::EngineReport,
            error::{ResolutionFailure, TemplateError},
            field_path::FieldPath,
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use std::{error::Error, path::Path, sync::Arc};

    fn location() -> TemplateLocation {
        TemplateLocation::new(Arc::from(Path::new("sensor.json5")), TemplateSite::Attribute(NameBuf::new("codes").unwrap()))
    }

    fn report(message: &str) -> EngineReport {
        EngineReport::unlocated(tera::Error::message(message))
    }

    #[test]
    fn a_failure_is_headlined_by_its_location_and_chains_the_hint_then_teras_report() {
        let error = TemplateError {
            location: location(),
            failure: Box::new(ResolutionFailure::NullField {
                template: TemplateSource::new("{{ code | split(pat=' ') }}"),
                field: FieldPath::new("code"),
                source: report("Invalid type for the value, expected `&str` but got `none`"),
            }),
        };

        assert_eq!(
            error.to_string(),
            "The attribute `codes` template in the mapping document at 'sensor.json5' could not be resolved"
        );
        let hint = error.source().expect("the hint is chained");
        assert_eq!(
            hint.to_string(),
            "`{{ code | split(pat=' ') }}` reads `code`, which is null in this record: guard it with `{% if code %}…{% endif %}` or default it with `code | default(value=…, boolean=true)`"
        );
        assert_eq!(
            hint.source().expect("the engine report is chained").to_string(),
            "Tera: Invalid type for the value, expected `&str` but got `none`"
        );
    }

    #[test]
    fn a_missing_field_is_named_with_a_guard_and_a_default() {
        let failure = ResolutionFailure::MissingField {
            template: TemplateSource::new("{{ n + 1 }}"),
            field: FieldPath::new("n"),
            source: report("`+` requires both operands to be numbers, found `undefined` and `i64`"),
        };

        assert_eq!(
            failure.to_string(),
            "`{{ n + 1 }}` reads `n`, which this record does not have: guard it with `{% if n %}…{% endif %}` or default it with `n | default(value=…)`"
        );
    }

    #[test]
    fn any_other_render_failure_names_the_template() {
        let failure = ResolutionFailure::Render {
            template: TemplateSource::new("{{ t | int }}"),
            source: report("boom"),
        };

        assert_eq!(failure.to_string(), "`{{ t | int }}` failed to render");
        assert_eq!(failure.source().unwrap().to_string(), "Tera: boom");
    }

    #[test]
    fn a_decode_failure_names_the_template_and_chains_the_parse_failure() {
        let failure = ResolutionFailure::Decode {
            template: TemplateSource::new("{{ x | split(pat=' ') }}"),
            source: serde_json::from_str::<serde_json::Value>("[\"BS\", ").unwrap_err(),
        };

        assert_eq!(failure.to_string(), "`{{ x | split(pat=' ') }}` did not render its value as JSON");
        assert!(failure.source().is_some());
    }
}
