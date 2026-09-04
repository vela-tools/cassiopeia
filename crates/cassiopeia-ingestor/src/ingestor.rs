use crate::error::IngestorError;
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::record::Record;

/// A data ingestor that reads from input sources and produces records.
///
/// Each implementation handles a specific data format (CSV, JSON, `GeoJSON`, KML).
/// The ingestor is consumed when `ingest` is called, sending batches of records
/// through the provided channel.
pub trait Ingestor: Send {
    /// Consume the ingestor and send records through the channel.
    ///
    /// The sender follows the run's channel policy: the default profile lets a fast ingestor run
    /// ahead for throughput, while a bounded profile blocks when the downstream channel is full.
    ///
    /// Implementations push `Signal::Data(Vec<Record>)` batches to the sender
    /// and return `Ok(())` when done.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the source cannot be parsed, a record's entity type is not a
    /// legal NGSI-LD name, or the downstream channel has closed.
    fn ingest(self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError>;
}
