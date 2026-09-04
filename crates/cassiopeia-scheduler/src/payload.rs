use cassiopeia_common::{
    run::{RunCount, RunNumber},
    signal::MetaSignal,
};

/// One scheduled run, emitted by the scheduler stage each time the pipeline should run.
#[derive(Debug, Clone)]
pub struct SchedulerPayload {
    /// 1-based index of this run within the schedule.
    pub run_number: RunNumber,
    /// Total number of runs when the schedule is bounded, `None` when it is unbounded.
    pub total_runs: Option<RunCount>,
}

impl SchedulerPayload {
    /// Converts the payload into the pipeline [`MetaSignal`] that carries run context downstream
    /// unchanged.
    #[must_use]
    pub const fn into_meta(self) -> MetaSignal {
        MetaSignal::ScheduleRun {
            run_number: self.run_number,
            total_runs: self.total_runs,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::payload::SchedulerPayload;
    use cassiopeia_common::{
        run::{RunCount, RunNumber},
        signal::MetaSignal,
    };

    #[test]
    fn a_payload_becomes_a_schedule_run_meta_signal() {
        let payload = SchedulerPayload {
            run_number: RunNumber::new(2),
            total_runs: Some(RunCount::new(5)),
        };

        assert_eq!(
            payload.into_meta(),
            MetaSignal::ScheduleRun {
                run_number: RunNumber::new(2),
                total_runs: Some(RunCount::new(5)),
            }
        );
    }
}
