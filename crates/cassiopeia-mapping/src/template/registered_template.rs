use crate::{
    template::{
        TemplateSource,
        error::{ResolutionFailure, TemplateError},
        template_name::TemplateName,
    },
    template_location::TemplateLocation,
};

/// A template registered with the Tera engine, with what a failure to resolve it is reported
/// against.
///
/// Tera knows the template only by its registration digest, and a value expression only in the
/// rewritten form it was registered as; neither is anything the author wrote. Keeping the source as
/// written and the declaration it came from beside the name is what lets a failure on any record
/// name the template and point at the mapping document and key that declare it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredTemplate {
    /// The name the template is registered under in the Tera engine.
    pub name: TemplateName,
    /// The template exactly as the mapping document wrote it.
    pub source: TemplateSource,
    /// Where the template is declared.
    pub location: TemplateLocation,
}

impl RegisteredTemplate {
    /// The error for this template failing to resolve, reported against its declaration.
    #[must_use]
    pub fn fail(&self, failure: ResolutionFailure) -> TemplateError {
        TemplateError {
            // The error outlives the record and the mapping this template is borrowed from.
            location: self.location.clone(),
            failure: Box::new(failure),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        template::{
            TemplateSource,
            engine_report::EngineReport,
            error::ResolutionFailure,
            registered_template::RegisteredTemplate,
            template_name::TemplateName,
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use std::{path::Path, sync::Arc};

    #[test]
    fn a_failure_is_reported_against_the_templates_declaration() {
        let template = RegisteredTemplate {
            name: TemplateName::for_source("{{ a | upper }}"),
            source: TemplateSource::new("{{ a | upper }}"),
            location: TemplateLocation::new(Arc::from(Path::new("sensor.json5")), TemplateSite::Scope),
        };

        let error = template.fail(ResolutionFailure::Render {
            template: template.source.clone(),
            source: EngineReport::unlocated(tera::Error::message("boom")),
        });

        assert_eq!(error.location, template.location);
        assert_eq!(
            error.to_string(),
            "The scope template in the mapping document at 'sensor.json5' could not be resolved"
        );
    }
}
