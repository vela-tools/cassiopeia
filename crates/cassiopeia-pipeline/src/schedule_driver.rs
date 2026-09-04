use crate::{
    error::{PipelineError, Result},
    pipeline::Pipeline,
    pipeline_stage::PipelineStage,
};
use cassiopeia_common::{
    run::RunCount,
    signal::{MetaSignal, Signal},
};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, run_code::RunCode},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_manifest::{failure_policy::FailurePolicy, schedule::Schedule};
use cassiopeia_reporter::reporter::{ProgressStage, StageOutput};
use cassiopeia_scheduler::{
    sleep::{SleepOutcome, interruptible_sleep},
    stage::spawn_scheduler_thread,
    wait::Wait,
};
use chrono::Local;
use std::{
    ops::ControlFlow,
    time::{Duration, Instant},
};

/// The minimum zero-padding width for a run number in the scheduled-run log lines.
const MIN_RUN_NUMBER_WIDTH: usize = 5;

impl Pipeline {
    /// Drives the pipeline through its schedule.
    ///
    /// A one-shot run (no schedule) pre-registers the stage progress bars and runs once; a repeating
    /// schedule suppresses the bars for compact per-run log lines. The run-level failure policy
    /// decides what a failed cycle does to the run: [`FailurePolicy::Abort`] stops at once,
    /// [`FailurePolicy::Continue`] and [`FailurePolicy::Ignore`] carry on to the remaining cycles.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`](crate::error::PipelineError) when a cycle fails under
    /// [`FailurePolicy::Abort`], or the first failure once every cycle has run under
    /// [`FailurePolicy::Continue`]. [`FailurePolicy::Ignore`] tolerates a failed cycle and returns
    /// `Ok`.
    pub(crate) fn run_schedule(&self, schedule: Option<Schedule>) -> Result<()> {
        let is_once = schedule.is_none();

        if is_once {
            let mut ordered: Vec<PipelineStage> = PipelineStage::all().to_vec();
            // A series run folds observations into temporal entities between the validator and the
            // writer, so the aggregator bar sits just before the writer's; a current-state run has no
            // aggregator stage.
            if self.output.series_representation
                && let Some(position) = ordered.iter().position(|stage| *stage == PipelineStage::Writer)
            {
                ordered.insert(position, PipelineStage::Aggregator);
            }
            let stages: Vec<Box<dyn ProgressStage>> = ordered.iter().map(|stage| Box::new(*stage) as Box<dyn ProgressStage>).collect();
            self.context.reporter.pre_register_stages(&stages);
        } else {
            self.context.reporter.set_quiet_stages(StageOutput::Quiet);
        }

        let scheduler_rx = spawn_scheduler_thread(schedule, self.context.reporter, self.context.shutdown);
        let start_time = Instant::now();
        let mut first_failure: Option<PipelineError> = None;

        for signal in scheduler_rx {
            match signal {
                Signal::Data(payload) => {
                    let run_start = Instant::now();
                    let run_number = payload.run_number.get();
                    let pad_width = run_number_width(payload.total_runs);
                    let schedule_meta = payload.into_meta();

                    match self.run_with_retry(&schedule_meta) {
                        Ok(()) => {
                            if !is_once {
                                let now = Local::now().format("%H:%M:%S");
                                self.context.reporter.success(&format!(
                                    "[{now}] #{run_number:0>pad_width$} completed ({:.2}s)",
                                    run_start.elapsed().as_secs_f64()
                                ));
                            }
                        }
                        Err(error) => {
                            let now = Local::now().format("%H:%M:%S");
                            // Built before the policy takes ownership of the failure, so the cycle's
                            // own line carries the failure's chain either way.
                            let diagnostic = DiagnosticBuilder::new(
                                Severity::Error,
                                DiagnosticCode::Run(RunCode::CycleFailed),
                                format!("[{now}] #{run_number:0>pad_width$} failed"),
                            )
                            .because(&error)
                            .build();
                            match record_cycle_failure(self.schedule.failure_policy, error, &mut first_failure) {
                                // Aborting propagates the failure, which the top of the process
                                // reports once with its whole chain; reporting it here as well would
                                // print it twice.
                                ControlFlow::Break(error) => return Err(error),
                                ControlFlow::Continue(()) => self.context.reporter.report(&diagnostic),
                            }
                        }
                    }
                }
                Signal::Stop => break,
                // The scheduler never emits an error: its channel error type is uninhabited.
                Signal::Error(never) => match never {},
                Signal::Start | Signal::Meta(_) => {}
            }
        }

        if !is_once {
            self.context
                .reporter
                .info(&format!("Schedule finished (total {:.1}s)", start_time.elapsed().as_secs_f64()));
        }

        // Under `Continue` the first failure is surfaced once every cycle has run; `Ignore` never
        // records one, so it returns `Ok`.
        match first_failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Runs one cycle, retrying it up to the retry policy's attempt count with an interruptible
    /// backoff between tries.
    fn run_with_retry(&self, schedule_meta: &MetaSignal) -> Result<()> {
        let max_attempts = self.schedule.retry.as_ref().map_or(1, |retry| retry.max_attempts().get());
        let backoff = self.schedule.retry.as_ref().map_or(Duration::ZERO, |retry| *retry.backoff());
        let mut attempt: u32 = 1;

        loop {
            match self.run_cycle(schedule_meta) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    if attempt >= max_attempts {
                        return Err(error);
                    }
                    self.context.reporter.report(
                        &DiagnosticBuilder::new(
                            Severity::Warning,
                            DiagnosticCode::Run(RunCode::CycleRetrying),
                            format!("Attempt {attempt}/{max_attempts} failed; retrying in {:.1}s", backoff.as_secs_f64()),
                        )
                        .because(&error)
                        .build(),
                    );
                    if interruptible_sleep(Wait::new(backoff), self.context.shutdown) == SleepOutcome::Interrupted {
                        return Err(error);
                    }
                    attempt += 1;
                }
            }
        }
    }
}

/// Applies the run-level failure policy to one failed cycle.
///
/// [`FailurePolicy::Abort`] breaks with the failure so the caller stops the run at once;
/// [`FailurePolicy::Continue`] records the first failure and carries on; [`FailurePolicy::Ignore`]
/// carries on without recording it, so the run exits `Ok`. A second `Continue` failure does not
/// overwrite the first: the run reports the earliest failure it saw.
fn record_cycle_failure(policy: FailurePolicy, error: PipelineError, first: &mut Option<PipelineError>) -> ControlFlow<PipelineError> {
    match policy {
        FailurePolicy::Abort => ControlFlow::Break(error),
        FailurePolicy::Continue => {
            first.get_or_insert(error);
            ControlFlow::Continue(())
        }
        FailurePolicy::Ignore => ControlFlow::Continue(()),
    }
}

/// The zero-padding width for a run number, wide enough for the total run count and never below
/// [`MIN_RUN_NUMBER_WIDTH`].
fn run_number_width(total_runs: Option<RunCount>) -> usize {
    match total_runs {
        Some(total) => usize::try_from(total.get().max(1).ilog10() + 1)
            .unwrap_or(MIN_RUN_NUMBER_WIDTH)
            .max(MIN_RUN_NUMBER_WIDTH),
        None => MIN_RUN_NUMBER_WIDTH,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::PipelineError,
        pipeline_stage::PipelineStage,
        schedule_driver::{record_cycle_failure, run_number_width},
    };
    use cassiopeia_common::run::RunCount;
    use cassiopeia_manifest::failure_policy::FailurePolicy;
    use std::ops::ControlFlow;

    /// A stand-in fatal failure whose variant the tests can match on.
    fn writer_panic() -> PipelineError {
        PipelineError::StagePanic { stage: PipelineStage::Writer }
    }

    /// A distinct fatal failure, used to prove a later failure does not overwrite the first.
    fn validator_panic() -> PipelineError {
        PipelineError::StagePanic {
            stage: PipelineStage::Validator,
        }
    }

    #[test]
    fn aborting_breaks_and_records_nothing() {
        let mut first = None;
        let flow = record_cycle_failure(FailurePolicy::Abort, writer_panic(), &mut first);

        assert!(matches!(flow, ControlFlow::Break(PipelineError::StagePanic { stage: PipelineStage::Writer })));
        assert!(first.is_none());
    }

    #[test]
    fn continuing_records_only_the_first_failure() {
        let mut first = None;

        assert!(matches!(
            record_cycle_failure(FailurePolicy::Continue, writer_panic(), &mut first),
            ControlFlow::Continue(())
        ));
        assert!(matches!(first, Some(PipelineError::StagePanic { stage: PipelineStage::Writer })));

        // A second failure carries on without overwriting the recorded first one.
        assert!(matches!(
            record_cycle_failure(FailurePolicy::Continue, validator_panic(), &mut first),
            ControlFlow::Continue(())
        ));
        assert!(matches!(first, Some(PipelineError::StagePanic { stage: PipelineStage::Writer })));
    }

    #[test]
    fn ignoring_carries_on_without_recording_a_failure() {
        let mut first = None;
        let flow = record_cycle_failure(FailurePolicy::Ignore, writer_panic(), &mut first);

        assert!(matches!(flow, ControlFlow::Continue(())));
        assert!(first.is_none());
    }

    #[test]
    fn a_small_or_unknown_run_total_pads_to_the_minimum_width() {
        assert_eq!(run_number_width(Some(RunCount::new(1))), 5);
        assert_eq!(run_number_width(Some(RunCount::new(9999))), 5);
        assert_eq!(run_number_width(None), 5);
    }

    #[test]
    fn a_large_run_total_widens_the_padding_to_fit() {
        assert_eq!(run_number_width(Some(RunCount::new(100_000))), 6);
        assert_eq!(run_number_width(Some(RunCount::new(1_000_000))), 7);
    }
}
