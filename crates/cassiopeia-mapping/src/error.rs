use crate::{template::compile_error::TemplateCompileError, template_site::TemplateSite};
use cassiopeia_common::error::io::IoError;
use cassiopeia_geometry::error::GeometryError;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use std::{path::PathBuf, result};
use thiserror::Error;

/// Failures raised while loading a mapping document.
#[derive(Debug, Error)]
pub enum MappingError {
    /// The mapping file could not be read from disk.
    #[error(transparent)]
    Io(#[from] IoError),

    /// The mapping document could not be parsed.
    #[error("Failed to parse the mapping document at '{}'", path.display())]
    Parse {
        /// The document that could not be parsed.
        path: PathBuf,
        /// The parse failure reported by the JSON5 reader.
        #[source]
        source: serde_json5::Error,
    },

    /// A template in the mapping is not valid template syntax, so no record could ever render it.
    #[error("The {site} template in the mapping document at '{}' cannot be compiled", path.display())]
    UncompilableTemplate {
        /// The document declaring the template.
        path: PathBuf,
        /// Where in the document the template is declared.
        site: TemplateSite,
        /// The compilation failure, naming the template and chaining the engine's own report. Boxed
        /// so this variant does not enlarge every `Result` carrying the error past the large-error
        /// threshold.
        #[source]
        source: Box<TemplateCompileError>,
    },

    /// An attribute declaration is internally inconsistent: its `geometry` conversion cannot
    /// produce the type its `transformation` names, so no record could ever satisfy it.
    #[error("Attribute `{attribute}` declares a geometry conversion that cannot run")]
    InvalidAttribute {
        /// The attribute whose declaration is inconsistent.
        attribute: NameBuf,
        /// Why the declared conversion cannot produce the declared type.
        #[source]
        source: GeometryError,
    },
}

/// The result type used throughout mapping loading.
pub type Result<T, E = MappingError> = result::Result<T, E>;

#[cfg(test)]
mod tests {
    use crate::{
        error::MappingError,
        template::{TemplateSource, compile_error::TemplateCompileError},
        template_site::TemplateSite,
    };
    use cassiopeia_common::error::io::{IoAction, IoError};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use std::{error::Error, io, path::PathBuf};

    #[test]
    fn an_uncompilable_template_names_the_document_and_the_site_and_chains_the_template() {
        let error = MappingError::UncompilableTemplate {
            path: PathBuf::from("/maps/sensor.json5"),
            site: TemplateSite::Attribute(NameBuf::new("temperature").unwrap()),
            source: Box::new(TemplateCompileError {
                template: TemplateSource::new("{{ t | upper "),
                source: tera::Error::message("Unexpected end of input"),
            }),
        };

        assert_eq!(
            error.to_string(),
            "The attribute `temperature` template in the mapping document at '/maps/sensor.json5' cannot be compiled"
        );
        let template = error.source().expect("the template failure is chained");
        assert!(template.to_string().contains("{{ t | upper "));
        assert_eq!(template.source().expect("the engine report is chained").to_string(), "Unexpected end of input");
    }

    #[test]
    fn an_io_failure_is_wrapped_transparently() {
        let io = IoError::FileOperation {
            source: io::Error::other("boom"),
            path: PathBuf::from("/maps/sensor.json5"),
            action: IoAction::Read,
        };

        assert!(MappingError::from(io).to_string().contains("sensor.json5"));
    }
}
