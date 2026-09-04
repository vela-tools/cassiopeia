use crate::error::ProfilerError;
use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
use cassiopeia_ir::payload::ProfiledPayload;
use std::collections::HashMap;

/// The routes a profiler sends profiled payloads down: one policy-controlled ingestor input channel
/// per format. Routing therefore follows the run's throughput or back-pressure policy.
pub type ProfilerRoutes = HashMap<DataFormat, ChannelSender<Signal<ProfiledPayload, ProfilerError>>>;

/// A profiler consumes collected payloads, determines each one's format, and routes it to the
/// matching ingestor channel.
///
/// The implementation owns the receiver it reads from (stored on the concrete type), so `profile`
/// takes only the outgoing `routes`. It consumes `self` because a profiler runs exactly once, to
/// exhaustion, on its own thread.
pub trait Profiler: Send {
    /// Reads collected payloads to completion, routing each profiled payload to its format's channel.
    ///
    /// # Errors
    ///
    /// Returns [`ProfilerError`] when a payload's format cannot be detected, no ingestor route is
    /// registered for it, or the ingestor channel has closed.
    fn profile(self: Box<Self>, routes: ProfilerRoutes) -> Result<(), ProfilerError>;
}
