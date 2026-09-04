use crate::template::error::TemplateError;
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

    /// A template in the mapping could not be evaluated.
    #[error(transparent)]
    Template(#[from] TemplateError),

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
    use crate::error::MappingError;
    use cassiopeia_common::error::io::{IoAction, IoError};
    use std::{io, path::PathBuf};

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
