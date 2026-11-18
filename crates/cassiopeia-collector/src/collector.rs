use crate::{collection_end::CollectionEnd, error::CollectorError};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::payload::CollectedPayload;

/// The Collector stage trait: handles data transport (downloading, reading files).
///
/// Implementations send one `Signal::Data(CollectedPayload)` per input source through the provided
/// policy-neutral sender, so collection follows the run's channel policy, and return how collection
/// ended: with what they met that the run carries on past, for the run to report, or stopped by the
/// run's stop.
pub trait Collector: Send {
    /// Collects all configured sources, sending each as a `Signal::Data` on `sender`, and returns
    /// how collection ended.
    ///
    /// # Errors
    ///
    /// Returns [`CollectorError`] when a source cannot be downloaded or read, or when the
    /// downstream channel has closed. A stop is no error: it ends collection with
    /// [`CollectionEnd::Stopped`].
    fn collect(self: Box<Self>, sender: ChannelSender<Signal<CollectedPayload, CollectorError>>) -> Result<CollectionEnd, CollectorError>;
}
