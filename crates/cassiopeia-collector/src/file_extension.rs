use crate::error::CollectorError;
use derive_more::Display;

/// A validated file extension used to name a downloaded source on disk.
///
/// The value names one path component (`input.<extension>`), so it must be a non-empty run of
/// characters that carries no path structure of its own: a dot would split the name, and a slash or
/// backslash would escape the temporary directory.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
#[display("{_0}")]
pub struct FileExtension(String);

impl FileExtension {
    /// Builds a file extension, rejecting any value that carries path structure.
    ///
    /// # Errors
    ///
    /// Returns [`CollectorError::InvalidFileExtension`] when `value` is empty or contains a dot,
    /// forward slash, or backslash.
    pub fn new(value: impl Into<String>) -> Result<FileExtension, CollectorError> {
        let value = value.into();
        if value.is_empty() || value.contains(['.', '/', '\\']) {
            return Err(CollectorError::InvalidFileExtension { value });
        }

        Ok(FileExtension(value))
    }

    /// The extension as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::CollectorError, file_extension::FileExtension};

    #[test]
    fn a_plain_extension_is_accepted() {
        assert_eq!(FileExtension::new("csv").unwrap().as_str(), "csv");
    }

    #[test]
    fn an_empty_extension_is_rejected() {
        assert!(matches!(FileExtension::new(""), Err(CollectorError::InvalidFileExtension { .. })));
    }

    #[test]
    fn an_extension_carrying_path_structure_is_rejected() {
        for value in ["ge.ojson", "a/b", "a\\b"] {
            assert!(matches!(FileExtension::new(value), Err(CollectorError::InvalidFileExtension { .. })));
        }
    }
}
