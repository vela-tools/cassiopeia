use crate::controller::RunController;
use cassiopeia_common::{
    channel::{ChannelPolicy, ChannelReceiver, ChannelSender, channel_with_telemetry},
    telemetry::{channel_boundary::ChannelBoundary, run::RunTelemetry},
};
use cassiopeia_reporter::reporter::Reporter;
use std::sync::Arc;

/// The execution environment shared by every cancellable pipeline stage.
///
/// It bundles the three values threaded into all of them: the channel back-pressure capacity, the
/// reporter, and the cancellation controller, so a stage's own signature carries only its
/// stage-specific parameters. Cloning it clones the controller `Arc` and copies the rest.
#[derive(Clone)]
pub(crate) struct StageEnv {
    /// Whether stage output is unbounded or applies fixed-capacity back-pressure.
    pub(crate) channel_policy: ChannelPolicy,
    /// The reporter the stage reports progress to.
    pub(crate) reporter: &'static dyn Reporter,
    /// The controller the stage polls for cancellation.
    pub(crate) controller: Arc<dyn RunController>,
    /// Per-cycle metrics shared by all workers and channels.
    pub(crate) telemetry: Arc<RunTelemetry>,
}

impl StageEnv {
    /// Creates a channel whose queue and wait metrics belong to this run, tagged with the stage
    /// boundary it bridges so the run summary can name it. The boundary is optional for the one
    /// caller (the run-bracketing scheduler) that maps to no measured stage.
    pub(crate) fn channel<T>(&self, boundary: Option<ChannelBoundary>) -> (ChannelSender<T>, ChannelReceiver<T>) {
        channel_with_telemetry(self.channel_policy, Some(Arc::clone(&self.telemetry)), boundary)
    }
}
