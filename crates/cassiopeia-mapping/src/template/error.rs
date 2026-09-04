use crate::template::template_name::TemplateName;
use std::result;
use thiserror::Error;

/// Failures raised while evaluating a compiled template against a source record.
#[derive(Debug, Error)]
pub enum TemplateError {
    /// A complex template failed to render in the Tera engine.
    #[error("The template `{template}` could not be rendered")]
    Render {
        /// The name the template is registered under in the Tera engine.
        template: TemplateName,
        /// The rendering failure reported by the Tera engine.
        #[source]
        source: tera::Error,
    },
}

/// The result type used throughout template evaluation.
pub type Result<T, E = TemplateError> = result::Result<T, E>;

#[cfg(test)]
mod tests {
    use crate::template::{error::TemplateError, template_name::TemplateName};
    use std::error::Error;

    #[test]
    fn a_render_error_names_the_template() {
        let error = TemplateError::Render {
            template: TemplateName::for_source("{{ x | upper }}"),
            source: tera::Error::message("boom"),
        };

        assert!(error.to_string().contains("tpl_"));
        // The renderer's own reason is the chained cause, not part of the headline: a message that
        // repeats its source prints it twice once the cause chain is rendered.
        assert_eq!(error.source().expect("a chained cause").to_string(), "boom");
    }
}
