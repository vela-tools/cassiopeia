use crate::{
    error::Result,
    run_outcome::RunOutcome,
    writer::{Writer, WriterStats},
};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use serde_jsonlines::{JsonLinesWriter, json_lines};
use std::{
    fs::File,
    io::{self, BufWriter},
    path::PathBuf,
};
use tempfile::NamedTempFile;
use thiserror::Error;

/// A failure operating the atomic writer's on-disk spool.
///
/// `serde-jsonlines` folds serialization and deserialization failures into [`io::Error`], so the
/// spool's read and write paths each carry a single error kind.
///
/// Creating the spool is its own variant precisely because it is the one step that runs before any
/// path exists: every later failure names the file it happened on rather than carrying an optional
/// path that is only ever absent in one case.
#[derive(Debug, Error)]
pub enum AtomicSpoolError {
    /// The temporary spool file could not be created.
    #[error("Failed to create the atomic writer spool file")]
    CreateSpool(#[source] io::Error),

    /// The freshly created spool file could not be reopened for writing.
    #[error("Failed to reopen the atomic writer spool at '{}'", path.display())]
    ReopenSpool {
        /// The underlying operating-system error.
        #[source]
        source: io::Error,
        /// The spool file.
        path: PathBuf,
    },

    /// An entity could not be serialized to, or flushed onto, the spool.
    #[error("Failed to write an entity to the atomic writer spool at '{}'", path.display())]
    Write {
        /// The underlying operating-system error.
        #[source]
        source: io::Error,
        /// The spool file.
        path: PathBuf,
    },

    /// The spool could not be read back, or a spooled line could not be deserialized.
    #[error("Failed to read the atomic writer spool at '{}'", path.display())]
    Read {
        /// The underlying operating-system error.
        #[source]
        source: io::Error,
        /// The spool file.
        path: PathBuf,
    },
}

/// A staging decorator that makes any [`Writer`] all-or-nothing.
///
/// Each entity is serialized losslessly and appended to a temporary NDJSON spool via
/// `serde-jsonlines`; no output reaches the inner writer until the pipeline finishes cleanly. On
/// [`RunOutcome::Committed`] the spool is replayed into the inner writer and the inner writer is
/// finalized; on [`RunOutcome::Aborted`] nothing is pushed and the inner writer is finalized so its
/// threads shut down cleanly. The spool is a [`NamedTempFile`], removed when this writer drops.
pub struct AtomicWriter {
    // Field order is drop order: the buffered writer drops (and flushes) before `_spool` removes the
    // temporary file, so no buffered bytes are lost to an already-unlinked path.
    /// The buffered NDJSON writer appending one entity per line to the spool.
    writer: JsonLinesWriter<BufWriter<File>>,
    /// Keeps the spool file alive and removes it on drop; held solely for its `Drop`.
    _spool: NamedTempFile,
    /// The spool's path, reopened for reading on commit.
    path: PathBuf,
    /// The wrapped writer that ultimately receives the entities.
    inner: Box<dyn Writer>,
}

impl AtomicWriter {
    /// Wraps `inner`, spooling entities to a fresh temporary file until commit.
    ///
    /// # Errors
    /// Returns a [`WriterError`](crate::error::WriterError) when the spool file cannot be created or
    /// reopened for writing.
    pub fn new(inner: Box<dyn Writer>) -> Result<AtomicWriter> {
        let spool = NamedTempFile::new().map_err(AtomicSpoolError::CreateSpool)?;
        let path = spool.path().to_path_buf();
        let handle = spool.reopen().map_err(|source| AtomicSpoolError::ReopenSpool {
            source,
            // The error owns the path after this borrowed handle is dropped.
            path: path.clone(),
        })?;

        Ok(AtomicWriter {
            writer: JsonLinesWriter::new(BufWriter::new(handle)),
            _spool: spool,
            path,
            inner,
        })
    }

    /// Replays every spooled entity into the inner writer, in write order.
    fn replay(&mut self) -> Result<()> {
        self.writer.flush().map_err(|source| self.write_failure(source))?;

        for entity in json_lines::<NgsiLdEntity, _>(&self.path).map_err(|source| AtomicSpoolError::Read {
            source,
            path: self.path.clone(),
        })? {
            let entity = entity.map_err(|source| AtomicSpoolError::Read {
                source,
                path: self.path.clone(),
            })?;
            self.inner.write(entity)?;
        }
        Ok(())
    }

    /// Names the spool a write failed on; the error owns the path because it outlives this writer.
    fn write_failure(&self, source: io::Error) -> AtomicSpoolError {
        AtomicSpoolError::Write {
            source,
            path: self.path.clone(),
        }
    }
}

impl Writer for AtomicWriter {
    fn write(&mut self, entity: NgsiLdEntity) -> Result<()> {
        self.writer.write(&entity).map_err(|source| self.write_failure(source))?;
        Ok(())
    }

    fn finalize(&mut self, outcome: RunOutcome) -> Result<WriterStats> {
        match outcome {
            RunOutcome::Committed => {
                self.replay()?;
                self.inner.finalize(RunOutcome::Committed)
            }
            // Push nothing; still finalize the inner writer so it can shut its threads down cleanly.
            RunOutcome::Aborted => self.inner.finalize(RunOutcome::Aborted),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        broker::atomic_writer::{AtomicSpoolError, AtomicWriter},
        error::Result,
        run_outcome::RunOutcome,
        writer::{Writer, WriterStats},
    };
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use std::{
        error::Error,
        io,
        path::PathBuf,
        sync::{Arc, Mutex, PoisonError},
        time::Duration,
    };
    use urn_rs::Urn;

    /// An inner writer that records every entity written to it and each finalize outcome, so a test
    /// can assert exactly what the atomic writer forwarded and when.
    #[derive(Default)]
    struct RecordingWriter {
        written: Arc<Mutex<Vec<NgsiLdEntity>>>,
        outcomes: Arc<Mutex<Vec<RunOutcome>>>,
    }

    impl Writer for RecordingWriter {
        fn write(&mut self, entity: NgsiLdEntity) -> Result<()> {
            self.written.lock().unwrap_or_else(PoisonError::into_inner).push(entity);
            Ok(())
        }

        fn finalize(&mut self, outcome: RunOutcome) -> Result<WriterStats> {
            self.outcomes.lock().unwrap_or_else(PoisonError::into_inner).push(outcome);
            let written = self.written.lock().unwrap_or_else(PoisonError::into_inner).len();
            Ok(WriterStats {
                written,
                failed: 0,
                bytes_written: 0,
                request_time: Duration::ZERO,
                retries: 0,
                backoff: Duration::ZERO,
                queue_wait: Duration::ZERO,
            })
        }
    }

    fn entity(id: &str) -> NgsiLdEntity {
        NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new("Sensor").unwrap())
    }

    #[test]
    fn nothing_reaches_the_inner_writer_before_a_commit() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let inner = RecordingWriter {
            written: Arc::clone(&written),
            outcomes: Arc::clone(&outcomes),
        };
        let mut writer = AtomicWriter::new(Box::new(inner)).unwrap();

        writer.write(entity("urn:ngsi-ld:Sensor:1")).unwrap();
        writer.write(entity("urn:ngsi-ld:Sensor:2")).unwrap();

        // Still spooled: the inner writer has seen nothing yet.
        assert!(written.lock().unwrap().is_empty());
    }

    #[test]
    fn a_commit_replays_every_spooled_entity_unchanged_in_order() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let inner = RecordingWriter {
            written: Arc::clone(&written),
            outcomes: Arc::clone(&outcomes),
        };
        let mut writer = AtomicWriter::new(Box::new(inner)).unwrap();

        let originals = vec![entity("urn:ngsi-ld:Sensor:1"), entity("urn:ngsi-ld:Sensor:2"), entity("urn:ngsi-ld:Sensor:3")];
        for original in &originals {
            writer.write(original.clone()).unwrap();
        }
        let stats = writer.finalize(RunOutcome::Committed).unwrap();

        assert_eq!(stats.written, 3);
        assert_eq!(*written.lock().unwrap(), originals);
        assert_eq!(*outcomes.lock().unwrap(), vec![RunOutcome::Committed]);
    }

    #[test]
    fn an_abort_pushes_nothing_but_still_finalizes_the_inner_writer() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let inner = RecordingWriter {
            written: Arc::clone(&written),
            outcomes: Arc::clone(&outcomes),
        };
        let mut writer = AtomicWriter::new(Box::new(inner)).unwrap();

        writer.write(entity("urn:ngsi-ld:Sensor:1")).unwrap();
        let stats = writer.finalize(RunOutcome::Aborted).unwrap();

        assert_eq!(stats.written, 0);
        assert!(written.lock().unwrap().is_empty());
        assert_eq!(*outcomes.lock().unwrap(), vec![RunOutcome::Aborted]);
    }

    #[test]
    fn a_spool_write_failure_names_the_spool_file() {
        let error = AtomicSpoolError::Write {
            source: io::Error::other("no space left on device"),
            path: PathBuf::from("/var/folders/tmp/.tmpAbC123"),
        };
        let message = error.to_string();

        assert!(message.contains("/var/folders/tmp/.tmpAbC123"));
        assert!(error.source().expect("a chained cause").to_string().contains("no space left"));
    }

    #[test]
    fn a_spool_creation_failure_has_no_path_to_name() {
        // Creating the spool is the one step that runs before any path exists.
        let error = AtomicSpoolError::CreateSpool(io::Error::other("permission denied"));

        assert!(error.to_string().contains("create the atomic writer spool"));
    }
}
