use crate::error::{PipelineError, Result};
use cassiopeia_collector::scratch_files::ScratchFiles;
use cassiopeia_reporter::reporter::MessageLog;
use std::{fs, io::ErrorKind, path::Path, sync::Arc};
use tempfile::Builder;

/// The scratch folder of one attempt at a cycle: the folder every collector of the attempt writes
/// what it downloads into.
///
/// The folder is created in the temporary folder when the attempt starts, and removed with everything
/// in it when the attempt ends, whether the attempt completed, failed or was stopped. A retried
/// attempt therefore starts from an empty folder, and nothing a cycle downloads outlives it. The
/// folder goes only once the attempt's output phase has ended, when the stages of a completed
/// attempt have read what its collectors handed on, so being the one owner of every download spares
/// those stages knowing who cleans up.
pub(crate) struct AttemptScratch {
    /// The folder, shared with every collector of the attempt.
    folder: Arc<Path>,
    /// Where a folder that cannot be removed is reported.
    log: &'static dyn MessageLog,
}

impl AttemptScratch {
    /// Creates the scratch folder of an attempt in `temporary_folder`, reporting to `log` when the
    /// folder cannot be removed once the attempt ends.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::ScratchFolder`] when the folder cannot be created.
    pub(crate) fn create(temporary_folder: &Path, log: &'static dyn MessageLog) -> Result<AttemptScratch> {
        let folder = Builder::new()
            .prefix("cassiopeia-")
            .tempdir_in(temporary_folder)
            .map_err(|source| PipelineError::ScratchFolder {
                folder: temporary_folder.to_path_buf(),
                source,
            })?;
        Ok(AttemptScratch {
            // The attempt removes the folder itself, so that a removal that fails is reported.
            folder: Arc::from(folder.keep()),
            log,
        })
    }

    /// The files the collector numbered `collector` among the attempt's collectors writes what it
    /// downloads to.
    pub(crate) fn files_of(&self, collector: u32) -> ScratchFiles {
        ScratchFiles::new(Arc::clone(&self.folder), collector)
    }
}

// Removing the folder is the attempt's cleanup, which no derive expresses.
impl Drop for AttemptScratch {
    fn drop(&mut self) {
        match fs::remove_dir_all(&self.folder) {
            Ok(()) => {}
            // A folder already gone holds nothing the run created.
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => self
                .log
                .info(&format!("The scratch folder '{}' could not be removed: {error}", self.folder.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{attempt_scratch::AttemptScratch, error::PipelineError, test_reporter::RecordingReporter};
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode};
    use cassiopeia_reporter::reporter::MessageLog;
    use pretty_assertions::assert_eq;
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    use tempfile::TempDir;

    /// A reporter the scratch folder reports to, which a test reads back.
    fn log() -> &'static RecordingReporter {
        Box::leak(Box::new(RecordingReporter::new()))
    }

    /// Every entry `folder` holds, in order.
    fn entries(folder: &Path) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = fs::read_dir(folder).unwrap().map(|entry| entry.unwrap().path()).collect();
        entries.sort();
        entries
    }

    #[test]
    fn an_attempt_creates_its_scratch_folder_in_the_temporary_folder_and_removes_it_with_everything_in_it_when_it_ends() {
        let temporary = TempDir::new().unwrap();
        let log = log();

        let scratch = AttemptScratch::create(temporary.path(), log as &'static dyn MessageLog).unwrap();
        let [folder] = <[PathBuf; 1]>::try_from(entries(temporary.path())).unwrap();
        assert!(folder.is_dir());
        fs::write(folder.join("input-0.csv"), "id\nS1\n").unwrap();
        fs::create_dir(folder.join("extracted")).unwrap();
        fs::write(folder.join("extracted").join("layer.shp"), "").unwrap();
        drop(scratch);

        assert_eq!(entries(temporary.path()), Vec::<PathBuf>::new());
        assert_eq!(log.messages(), Vec::<String>::new());
    }

    #[test]
    fn an_attempt_whose_folder_is_already_gone_ends_without_a_word() {
        let temporary = TempDir::new().unwrap();
        let log = log();

        let scratch = AttemptScratch::create(temporary.path(), log as &'static dyn MessageLog).unwrap();
        let [folder] = <[PathBuf; 1]>::try_from(entries(temporary.path())).unwrap();
        fs::remove_dir(folder).unwrap();
        drop(scratch);

        assert_eq!(log.messages(), Vec::<String>::new());
    }

    #[test]
    fn two_attempts_never_share_a_scratch_folder() {
        let temporary = TempDir::new().unwrap();
        let log = log() as &'static dyn MessageLog;

        let first = AttemptScratch::create(temporary.path(), log).unwrap();
        let second = AttemptScratch::create(temporary.path(), log).unwrap();

        assert_eq!(entries(temporary.path()).len(), 2);
        assert_ne!(first.files_of(0), second.files_of(0));
    }

    #[test]
    fn the_collectors_of_one_attempt_are_handed_files_apart() {
        let temporary = TempDir::new().unwrap();

        let scratch = AttemptScratch::create(temporary.path(), log() as &'static dyn MessageLog).unwrap();

        assert_ne!(scratch.files_of(0), scratch.files_of(1));
    }

    #[test]
    fn an_attempt_whose_temporary_folder_is_missing_fails_naming_it_as_a_source_that_cannot_be_collected() {
        let temporary = TempDir::new().unwrap();
        let missing = temporary.path().join("missing");

        let Err(error) = AttemptScratch::create(&missing, log() as &'static dyn MessageLog) else {
            panic!("a scratch folder cannot be created in a folder that does not exist");
        };

        assert!(matches!(&error, PipelineError::ScratchFolder { folder, .. } if *folder == missing), "{error:?}");
        assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Ingest(IngestCode::SourceUnavailable));
        assert!(error.to_string().contains(&missing.display().to_string()), "{error}");
    }
}
