use crate::template::TemplateSource;
use thiserror::Error;

/// A template expression the Tera engine refused to register, raised once while a mapping is loaded.
///
/// Kept apart from [`TemplateError`](crate::template::error::TemplateError), which describes a
/// compiled template failing against one record: a compile failure means no record could ever
/// render the template, so it rejects the mapping rather than dropping one attribute of one entity.
#[derive(Debug, Error)]
#[error("The template `{template}` is not valid template syntax")]
pub struct TemplateCompileError {
    /// The expression exactly as the mapping document wrote it.
    pub template: TemplateSource,
    /// The engine's own report, pointing at the offending span: a syntax error, or a filter,
    /// function, or test that is not registered.
    #[source]
    pub source: tera::Error,
}

#[cfg(test)]
mod tests {
    use crate::template::{TemplateSource, compile_error::TemplateCompileError};
    use std::error::Error;

    #[test]
    fn a_compile_error_names_the_template_as_written_and_chains_the_engine_report() {
        let error = TemplateCompileError {
            template: TemplateSource::new("{{ a | upper "),
            source: tera::Error::message("Unexpected end of input"),
        };

        assert_eq!(error.to_string(), "The template `{{ a | upper ` is not valid template syntax");
        // The engine's report is the chained cause rather than part of the headline, so rendering
        // the cause chain prints it once.
        assert_eq!(error.source().expect("a chained cause").to_string(), "Unexpected end of input");
    }
}
