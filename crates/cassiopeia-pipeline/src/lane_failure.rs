use crate::error::{PipelineError, Result};
use cassiopeia_common::channel::{ChannelPolicy, ChannelReceiver, ChannelSender, channel};
use std::sync::mpsc::RecvError;

/// The path one input lane reports its failure through.
///
/// Every thread of a lane that can fail holds a clone, so the lane has ended once the last clone is
/// dropped: the [`LaneEndings`] waiting on it can then tell a lane that ended cleanly from one that
/// is still running.
#[derive(Clone)]
pub(crate) struct LaneFailure(ChannelSender<PipelineError>);

impl LaneFailure {
    /// Reports `error` as the lane's failure. Only the lane's first failure is read; a later one
    /// stays unread.
    pub(crate) fn report(&self, error: PipelineError) {
        // The endings stop listening only once the cycle has already failed on an earlier failure.
        let _ = self.0.send(error);
    }
}

/// How the input lanes of a cycle end, in the order the manifest lists their inputs.
///
/// The lanes run concurrently, so the order in which their failures happen depends on how their
/// threads are scheduled. Reading the failures in manifest order instead makes the failure a cycle
/// reports the same on every run: the failure of the first input that fails.
#[derive(Default)]
pub(crate) struct LaneEndings(Vec<ChannelReceiver<PipelineError>>);

impl LaneEndings {
    /// Opens the failure path of the next lane in manifest order.
    pub(crate) fn open(&mut self) -> LaneFailure {
        // A lane reports a failure or two at most, so its path never holds a stage back.
        let (sender, receiver) = channel(ChannelPolicy::Unbounded);
        self.0.push(receiver);
        LaneFailure(sender)
    }

    /// Waits for the lanes in manifest order and returns the failure of the first lane that fails,
    /// as soon as every lane before it has ended without one; a lane after it is not waited for.
    ///
    /// # Errors
    ///
    /// Returns the first failure the first failing lane reported.
    pub(crate) fn first_failure(self) -> Result<()> {
        self.0.into_iter().try_for_each(|lane| match lane.recv() {
            Ok(failure) => Err(failure),
            // Every thread of the lane dropped its handle without reporting: it ended cleanly.
            Err(RecvError) => Ok(()),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::PipelineError, lane_failure::LaneEndings, pipeline_stage::PipelineStage};

    /// A failure that names `stage`, so a test can tell which lane's failure was returned.
    const fn failure(stage: PipelineStage) -> PipelineError {
        PipelineError::StagePanic { stage }
    }

    #[test]
    fn lanes_that_end_without_a_failure_give_none() {
        let mut endings = LaneEndings::default();
        drop(endings.open());
        drop(endings.open());

        assert!(endings.first_failure().is_ok());
    }

    #[test]
    fn the_failure_of_the_first_lane_in_manifest_order_wins_over_one_a_later_lane_reported_earlier() {
        let mut endings = LaneEndings::default();
        let first = endings.open();
        let second = endings.open();
        second.report(failure(PipelineStage::Profiler));
        first.report(failure(PipelineStage::Collector));
        drop((first, second));

        let error = endings.first_failure().unwrap_err();

        assert!(
            matches!(
                error,
                PipelineError::StagePanic {
                    stage: PipelineStage::Collector
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn a_later_lane_s_failure_is_returned_once_every_earlier_lane_ended_without_one() {
        let mut endings = LaneEndings::default();
        let first = endings.open();
        let second = endings.open();
        second.report(failure(PipelineStage::Profiler));
        drop((first, second));

        let error = endings.first_failure().unwrap_err();

        assert!(
            matches!(
                error,
                PipelineError::StagePanic {
                    stage: PipelineStage::Profiler
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn a_failure_is_returned_without_waiting_for_a_later_lane_still_running() {
        let mut endings = LaneEndings::default();
        let first = endings.open();
        let still_running = endings.open();
        first.report(failure(PipelineStage::Collector));

        let error = endings.first_failure().unwrap_err();

        assert!(
            matches!(
                error,
                PipelineError::StagePanic {
                    stage: PipelineStage::Collector
                }
            ),
            "{error}"
        );
        drop((first, still_running));
    }

    #[test]
    fn a_lane_reports_its_first_failure() {
        let mut endings = LaneEndings::default();
        let lane = endings.open();
        // The lane's other threads keep a handle of their own, as a lane's stages do.
        let other_thread = lane.clone();
        lane.report(failure(PipelineStage::Collector));
        other_thread.report(failure(PipelineStage::Ingestor));
        drop((lane, other_thread));

        let error = endings.first_failure().unwrap_err();

        assert!(
            matches!(
                error,
                PipelineError::StagePanic {
                    stage: PipelineStage::Collector
                }
            ),
            "{error}"
        );
    }
}
