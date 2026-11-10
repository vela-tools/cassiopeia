use crate::error::{DataProfilerError, Result};
use cassiopeia_common::error::io::{IoAction, IoError};
use memmap2::Mmap;
use std::{fs::File, io::ErrorKind, path::Path};

/// A file mapped read-only into memory for inspection.
///
/// Mapping rather than reading into a heap buffer keeps resident memory proportional to the bytes an
/// inspection actually touches: a magic-number check faults in only the leading page, a whole-file
/// scan (JSON validation, a CSV sample, an encoding guess) streams through clean read-only pages the
/// kernel can evict under pressure, and an inspection that looks at nothing reads nothing. For a
/// multi-gigabyte input that is the difference between a few pages and an out-of-memory abort.
///
/// Both the auto-detect path and the declared-format path open a file through this type, so neither
/// reads more of a file than its detectors or inspector need. The mapping is meant to be held only
/// while the file is inspected, not for the ingestion that follows.
#[derive(Debug)]
pub struct MappedFile {
    /// The read-only mapping of the whole file.
    mapping: Mmap,
}

impl MappedFile {
    /// Opens the file at `path` and maps it read-only.
    ///
    /// An empty file maps to no bytes rather than failing, so it reaches its detectors or inspector
    /// like any other content.
    ///
    /// # Errors
    /// Returns [`DataProfilerError::NotFound`] if the path does not exist, or
    /// [`DataProfilerError::Io`] if the file cannot be opened or mapped.
    pub fn open(path: &Path) -> Result<MappedFile> {
        let file = File::open(path).map_err(|source| {
            if source.kind() == ErrorKind::NotFound {
                DataProfilerError::NotFound(path.to_path_buf())
            } else {
                DataProfilerError::Io(IoError::FileOperation {
                    source,
                    path: path.to_path_buf(),
                    action: IoAction::Open,
                })
            }
        })?;

        // SAFETY: the mapping is read-only and is held only while the file is inspected, during
        // which Cassiopeia never writes to the path. The residual hazard is another process
        // truncating the file underneath the mapping; that is the accepted trade-off for inspecting
        // large inputs without loading them, the same one made by ripgrep and fd.
        let mapping = unsafe { Mmap::map(&file) }.map_err(|source| IoError::FileOperation {
            source,
            path: path.to_path_buf(),
            action: IoAction::Read,
        })?;
        Ok(MappedFile { mapping })
    }

    /// The file's content, read from disk only as it is touched.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.mapping
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::DataProfilerError, mapped_file::MappedFile};
    use pretty_assertions::assert_eq;
    use std::io::Write;
    use tempfile::{NamedTempFile, tempdir};

    #[test]
    fn a_mapped_file_exposes_the_bytes_on_disk() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"id;name\n1;alpha\n").unwrap();

        let mapped = MappedFile::open(file.path()).unwrap();

        assert_eq!(mapped.bytes(), b"id;name\n1;alpha\n");
    }

    #[test]
    fn an_empty_file_maps_to_no_bytes() {
        let file = NamedTempFile::new().unwrap();

        let mapped = MappedFile::open(file.path()).unwrap();

        assert_eq!(mapped.bytes(), b"");
    }

    #[test]
    fn a_missing_path_is_reported_as_not_found() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("absent.csv");

        let error = MappedFile::open(&path).unwrap_err();

        assert!(matches!(error, DataProfilerError::NotFound(missing) if missing == path));
    }

    #[test]
    fn a_directory_is_an_io_failure_not_a_missing_file() {
        let directory = tempdir().unwrap();

        let error = MappedFile::open(directory.path()).unwrap_err();

        assert!(matches!(error, DataProfilerError::Io(_)));
    }
}
