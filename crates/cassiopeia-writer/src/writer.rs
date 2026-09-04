use crate::{error::Result, run_outcome::RunOutcome};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use std::time::Duration;

/// The tally a writer returns once every entity has been flushed.
#[derive(Debug, Clone, Copy)]
pub struct WriterStats {
    /// How many entities reached the destination.
    pub written: usize,
    /// How many entities were dropped after exhausting retries.
    pub failed: usize,
    /// Bytes confirmed written or delivered, excluding failed attempts.
    pub bytes_written: u64,
    /// Aggregate request time, when the destination exposes it.
    pub request_time: Duration,
    /// Number of retry attempts made by the destination.
    pub retries: u64,
    /// Time spent in retry backoff.
    pub backoff: Duration,
    /// Time spent waiting for the destination delivery queue.
    pub queue_wait: Duration,
}

/// Confirmed writer progress available before finalization.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriterProgress {
    /// Entities confirmed written or delivered so far.
    pub written: usize,
    /// Entities failed so far after the writer's retry policy.
    pub failed: usize,
    /// Bytes confirmed written or delivered so far.
    pub bytes_written: u64,
}

/// Emits NGSI-LD entities to a destination (a directory of files, or a Context Broker).
///
/// A writer is stateful and single-owner (`Send`, not `Sync`): the pipeline holds one and feeds it,
/// then calls [`finalize`](Writer::finalize) exactly once to flush and collect statistics.
pub trait Writer: Send {
    /// Writes a single entity.
    ///
    /// # Errors
    /// Returns a [`WriterError`](crate::error::WriterError) when the entity cannot be serialized or
    /// emitted to the destination.
    fn write(&mut self, entity: NgsiLdEntity) -> Result<()>;

    /// Returns confirmed progress without flushing additional buffered output.
    fn progress(&self) -> WriterProgress {
        WriterProgress::default()
    }

    /// Writes a batch of entities.
    ///
    /// The default implementation writes them one at a time; an implementation can override it to
    /// batch or parallelize the work.
    ///
    /// # Errors
    /// Returns a [`WriterError`](crate::error::WriterError) when any entity in the batch cannot be
    /// written.
    fn write_batch(&mut self, entities: Vec<NgsiLdEntity>) -> Result<()> {
        for entity in entities {
            self.write(entity)?;
        }
        Ok(())
    }

    /// Flushes any buffered output and returns the final statistics.
    ///
    /// `outcome` reports whether the run reached this point cleanly ([`RunOutcome::Committed`]) or
    /// after a failure or cancellation ([`RunOutcome::Aborted`]). A streaming writer has already
    /// emitted its output and ignores it; a staging writer commits or discards its spool on it.
    ///
    /// # Errors
    /// Returns a [`WriterError`](crate::error::WriterError) when buffered output cannot be flushed.
    fn finalize(&mut self, outcome: RunOutcome) -> Result<WriterStats>;
}

#[cfg(test)]
mod tests {
    use crate::{
        error::Result,
        run_outcome::RunOutcome,
        writer::{Writer, WriterStats},
    };
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use std::time::Duration;
    use urn_rs::Urn;

    /// A writer that counts every entity it is handed, to exercise the default batch method.
    #[derive(Default)]
    struct CountingWriter {
        written: usize,
    }

    impl Writer for CountingWriter {
        fn write(&mut self, _entity: NgsiLdEntity) -> Result<()> {
            self.written += 1;
            Ok(())
        }

        fn finalize(&mut self, _outcome: RunOutcome) -> Result<WriterStats> {
            Ok(WriterStats {
                written: self.written,
                failed: 0,
                bytes_written: 0,
                request_time: Duration::ZERO,
                retries: 0,
                backoff: Duration::ZERO,
                queue_wait: Duration::ZERO,
            })
        }
    }

    #[test]
    fn the_default_batch_writes_every_entity() {
        let mut writer = CountingWriter::default();
        let entities = vec![
            NgsiLdEntity::new("urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap(), NameBuf::new("Sensor").unwrap()),
            NgsiLdEntity::new("urn:ngsi-ld:Sensor:2".parse::<Urn>().unwrap(), NameBuf::new("Sensor").unwrap()),
        ];

        writer.write_batch(entities).unwrap();

        assert_eq!(writer.finalize(RunOutcome::Committed).unwrap().written, 2);
    }
}
