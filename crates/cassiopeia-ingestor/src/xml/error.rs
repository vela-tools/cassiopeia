use quick_xml::events::attributes::AttrError;
use std::{io, path::PathBuf};

/// Errors that can occur while parsing XML input into records.
///
/// The parser never expands DTD or external entities: [`quick_xml`] surfaces any unknown custom
/// entity as [`XmlIngestError::UnknownEntity`] rather than resolving it, so this ingestor is XXE and
/// billion-laughs safe by construction.
#[derive(Debug, thiserror::Error)]
pub enum XmlIngestError {
    /// The XML ingestor was handed an in-memory byte payload; it needs a file.
    #[error("the XML ingestor requires a file, not bytes")]
    RequiresFile,

    /// Reading the file into memory failed.
    #[error("failed to read the XML input at '{}'", path.display())]
    Read {
        /// The underlying operating-system error.
        #[source]
        source: io::Error,
        /// The file that could not be read.
        path: PathBuf,
    },

    /// The input could not be parsed as well-formed XML, or a value could not be decoded.
    #[error(transparent)]
    Parse(#[from] quick_xml::Error),

    /// An element attribute could not be parsed.
    #[error(transparent)]
    Attr(#[from] AttrError),

    /// The input bytes were not valid for the encoding named by a BOM or the XML declaration.
    #[error("input is not valid {0}")]
    Decode(&'static str),

    /// The document referenced a custom entity, which is never expanded.
    #[error("unknown XML entity: {0}")]
    UnknownEntity(String),

    /// The input contained no root element.
    #[error("the XML document had no root element")]
    EmptyDocument,
}

#[cfg(test)]
mod tests {
    use crate::xml::error::XmlIngestError;
    use std::{error::Error, io, path::PathBuf};

    #[test]
    fn a_read_failure_names_the_file_and_keeps_the_operating_system_reason() {
        let error = XmlIngestError::Read {
            source: io::Error::other("input/output error"),
            path: PathBuf::from("/data/observations.xml"),
        };

        assert!(error.to_string().contains("/data/observations.xml"));
        assert_eq!(error.source().expect("a chained cause").to_string(), "input/output error");
    }
}
