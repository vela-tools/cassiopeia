use crate::error::IngestorError;
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::record::Record;
use std::mem;

/// Groups records into batches of the run's batch size and sends each full batch downstream.
pub struct RecordBatches<'a> {
    sender: &'a ChannelSender<Signal<Vec<Record>, IngestorError>>,
    batch_size: usize,
    buffer: Vec<Record>,
}

impl<'a> RecordBatches<'a> {
    /// Opens an empty batch that sends through `sender` once it holds `batch_size` records.
    #[must_use]
    pub const fn new(sender: &'a ChannelSender<Signal<Vec<Record>, IngestorError>>, batch_size: usize) -> RecordBatches<'a> {
        RecordBatches {
            sender,
            batch_size,
            buffer: Vec::new(),
        }
    }

    /// Adds one record, sending the batch when it is full.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError::ChannelClosed`] when the downstream channel has closed.
    pub fn push(&mut self, record: Record) -> Result<(), IngestorError> {
        self.buffer.push(record);
        if self.buffer.len() >= self.batch_size {
            let batch = mem::take(&mut self.buffer);
            self.sender.send(Signal::Data(batch)).map_err(|_| IngestorError::ChannelClosed)?;
        }
        Ok(())
    }

    /// Sends the records of the last, partly filled batch.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError::ChannelClosed`] when the downstream channel has closed.
    pub fn finish(self) -> Result<(), IngestorError> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        self.sender.send(Signal::Data(self.buffer)).map_err(|_| IngestorError::ChannelClosed)
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::IngestorError, record_batches::RecordBatches};
    use cassiopeia_common::{channel::ChannelSender, signal::Signal};
    use cassiopeia_ir::record::Record;
    use pretty_assertions::assert_eq;
    use serde_json::Map;
    use std::sync::mpsc::sync_channel;

    fn batch_sizes(records: usize, batch_size: usize) -> Vec<usize> {
        let (tx, rx) = sync_channel(records + 1);
        let sender = ChannelSender::bounded(tx);
        let mut batches = RecordBatches::new(&sender, batch_size);
        for _ in 0..records {
            batches.push(Record::new(None, Map::new())).unwrap();
        }
        batches.finish().unwrap();
        drop(sender);

        rx.iter()
            .map(|signal: Signal<Vec<Record>, IngestorError>| match signal {
                Signal::Data(batch) => batch.len(),
                Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => panic!("expected a data signal"),
            })
            .collect()
    }

    #[test]
    fn records_are_sent_in_full_batches_followed_by_the_remainder() {
        assert_eq!(batch_sizes(5, 2), vec![2, 2, 1]);
    }

    #[test]
    fn an_exact_multiple_of_the_batch_size_sends_no_empty_trailing_batch() {
        assert_eq!(batch_sizes(4, 2), vec![2, 2]);
    }

    #[test]
    fn no_records_send_no_batch() {
        assert_eq!(batch_sizes(0, 2), Vec::<usize>::new());
    }

    #[test]
    fn a_closed_channel_is_reported() {
        let (tx, rx) = sync_channel(1);
        drop(rx);
        let sender = ChannelSender::bounded(tx);
        let mut batches = RecordBatches::new(&sender, 1);

        assert!(matches!(batches.push(Record::new(None, Map::new())), Err(IngestorError::ChannelClosed)));
    }
}
