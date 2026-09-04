use crate::error::CollectorError;
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::payload::CollectedPayload;

/// The Collector stage trait: handles data transport (downloading, reading files).
///
/// Implementations send one `Signal::Data(CollectedPayload)` per input source through the provided
/// policy-neutral sender, so collection follows the run's channel policy.
pub trait Collector: Send {
    /// Collects all configured sources, sending each as a `Signal::Data` on `sender`.
    ///
    /// # Errors
    ///
    /// Returns [`CollectorError`] when a source cannot be downloaded or read, or when the
    /// downstream channel has closed.
    fn collect(self: Box<Self>, sender: ChannelSender<Signal<CollectedPayload, CollectorError>>) -> Result<(), CollectorError>;
}
