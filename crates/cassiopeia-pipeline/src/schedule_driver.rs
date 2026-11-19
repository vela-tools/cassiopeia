use crate::{
    cycle_outcome::CycleOutcome,
    error::{PipelineError, Result},
    pipeline::Pipeline,
    pipeline_stage::PipelineStage,
    scheduled_run::ScheduledRun,
};
use cassiopeia_common::{cycle_start::CycleStart, run::RunCount, signal::Signal};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, run_code::RunCode},
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_manifest::{failure_policy::FailurePolicy, schedule::Schedule};
use cassiopeia_reporter::reporter::{ProgressStage, StageOutput};
use cassiopeia_scheduler::{
    payload::SchedulerPayload,
    sleep::{SleepOutcome, interruptible_sleep},
    stage::spawn_scheduler_thread,
    wait::Wait,
};
use chrono::Local;
use std::{
    ops::ControlFlow,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

/// The minimum zero-padding width for a run number in the scheduled-run log lines.
const MIN_RUN_NUMBER_WIDTH: usize = 5;

/// Whether a run fires one cycle or repeats on a schedule, which decides how its cycles are
/// reported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Repetition {
    /// The manifest declares no schedule: one cycle, with stage progress bars.
    Once,
    /// The cycles repeat on the manifest's schedule, each reported on a line of its own.
    Scheduled,
}

impl Pipeline {
    /// Drives the pipeline through its schedule.
    ///
    /// A one-shot run (no schedule) pre-registers the stage progress bars and runs once; a repeating
    /// schedule suppresses the bars for compact per-run log lines. The run-level failure policy
    /// decides what a failed cycle does to the run: [`FailurePolicy::Abort`] stops at once,
    /// [`FailurePolicy::Continue`] and [`FailurePolicy::Ignore`] carry on to the remaining cycles.
    /// A stopped cycle is no failure: the run reports it and ends, as the stop asked.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`](crate::error::PipelineError) when a cycle fails under
    /// [`FailurePolicy::Abort`], or the first failure once every cycle has run under
    /// [`FailurePolicy::Continue`]. [`FailurePolicy::Ignore`] tolerates a failed cycle and returns
    /// `Ok`.
    pub(crate) fn run_schedule(&self, schedule: Option<Schedule>) -> Result<()> {
        let repetition = if schedule.is_some() { Repetition::Scheduled } else { Repetition::Once };

        match repetition {
            Repetition::Once => {
                let mut ordered: Vec<PipelineStage> = PipelineStage::all().to_vec();
                // A series run folds observations into temporal entities between the validator and
                // the writer, so the aggregator bar sits just before the writer's; a current-state
                // run has no aggregator stage.
                if self.output.is_series()
                    && let Some(position) = ordered.iter().position(|stage| *stage == PipelineStage::Writer)
                {
                    ordered.insert(position, PipelineStage::Aggregator);
                }
                let stages: Vec<Box<dyn ProgressStage>> = ordered.iter().map(|stage| Box::new(*stage) as Box<dyn ProgressStage>).collect();
                self.context.reporter.pre_register_stages(&stages);
            }
            Repetition::Scheduled => self.context.reporter.set_quiet_stages(StageOutput::Quiet),
        }

        let scheduler_rx = spawn_scheduler_thread(schedule, self.context.reporter, self.context.shutdown);
        let start_time = Instant::now();
        let mut first_failure: Option<PipelineError> = None;

        for signal in scheduler_rx {
            match signal {
                Signal::Data(payload) => {
                    // The scheduler hands its next cycle over on a rendezvous, so a cycle that fell
                    // due while the one before ran is handed over once that one ends, even when the
                    // run is being stopped by then. A scheduled run that is being stopped starts no
                    // further cycle.
                    if repetition == Repetition::Scheduled && self.context.shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    if let ControlFlow::Break(error) = self.run_handed_cycle(payload, repetition, &mut first_failure) {
                        return Err(error);
                    }
                }
                Signal::Stop => break,
                // The scheduler never emits an error: its channel error type is uninhabited.
                Signal::Error(never) => match never {},
                Signal::Start | Signal::Meta(_) => {}
            }
        }

        if repetition == Repetition::Scheduled {
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

    /// Runs the cycle the scheduler handed over as `payload` and reports how it ended.
    ///
    /// A scheduled run reports every cycle on a line of its own: completed, stopped or failed. A
    /// one-shot run reports a stopped or failed cycle only, since its summary already shows what a
    /// completed one did. A failed cycle goes to the run-level failure policy, and the flow breaks
    /// with the failure when the run aborts; a stopped one never breaks it, since a stop is no
    /// failure.
    fn run_handed_cycle(&self, payload: SchedulerPayload, repetition: Repetition, first_failure: &mut Option<PipelineError>) -> ControlFlow<PipelineError> {
        let run_start = Instant::now();
        let run_number = payload.run_number.get();
        let pad_width = run_number_width(payload.total_runs);
        // The run's start is taken once, before its first attempt, so every attempt builds the same
        // requests.
        let run = ScheduledRun {
            meta: payload.into_meta(),
            start: CycleStart::now(),
        };

        let outcome = self.run_with_retry(&run);
        let seconds = run_start.elapsed().as_secs_f64();
        match (outcome, repetition) {
            (Ok(CycleOutcome::Completed), Repetition::Scheduled) => {
                self.context
                    .reporter
                    .success(&format!("{} completed ({seconds:.2}s)", cycle_label(run_number, pad_width)));
            }
            (Ok(CycleOutcome::Completed), Repetition::Once) => {}
            (Ok(CycleOutcome::Stopped), Repetition::Scheduled) => {
                self.context
                    .reporter
                    .info(&format!("{} stopped ({seconds:.2}s)", cycle_label(run_number, pad_width)));
            }
            (Ok(CycleOutcome::Stopped), Repetition::Once) => self.context.reporter.info(&format!("Run stopped ({seconds:.2}s)")),
            (Err(error), Repetition::Scheduled | Repetition::Once) => {
                // Built before the policy takes ownership of the failure, so the cycle's own line
                // carries the failure's chain either way.
                let diagnostic = DiagnosticBuilder::new(
                    Severity::Error,
                    DiagnosticCode::Run(RunCode::CycleFailed),
                    format!("{} failed", cycle_label(run_number, pad_width)),
                )
                .because(&error)
                .build();
                match record_cycle_failure(self.schedule.failure_policy, error, first_failure) {
                    // Aborting propagates the failure, which the top of the process reports once
                    // with its whole chain; reporting it here as well would print it twice.
                    ControlFlow::Break(error) => return ControlFlow::Break(error),
                    ControlFlow::Continue(()) => self.context.reporter.report(&diagnostic),
                }
            }
        }
        ControlFlow::Continue(())
    }

    /// Runs one cycle, retrying it up to the retry policy's attempt count with an interruptible
    /// backoff between tries. A stopped cycle is not retried, and a stop during the backoff stops
    /// the cycle: the failed attempt is already reported as retrying, and a stop is no failure.
    fn run_with_retry(&self, run: &ScheduledRun) -> Result<CycleOutcome> {
        let max_attempts = self.schedule.retry.as_ref().map_or(1, |retry| retry.max_attempts().get());
        let backoff = self.schedule.retry.as_ref().map_or(Duration::ZERO, |retry| *retry.backoff());
        let mut attempt: u32 = 1;

        loop {
            match self.run_cycle(run) {
                Ok(outcome) => return Ok(outcome),
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
                        return Ok(CycleOutcome::Stopped);
                    }
                    attempt += 1;
                }
            }
        }
    }
}

/// The local time and the zero-padded number, `pad_width` digits wide, that open the line of the
/// scheduled cycle `run_number`, such as `[14:18:14] #00001`.
fn cycle_label(run_number: u32, pad_width: usize) -> String {
    format!("[{}] #{run_number:0>pad_width$}", Local::now().format("%H:%M:%S"))
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
        schedule_driver::{cycle_label, record_cycle_failure, run_number_width},
    };
    use cassiopeia_common::run::RunCount;
    use cassiopeia_manifest::failure_policy::FailurePolicy;
    use chrono::NaiveTime;
    use pretty_assertions::assert_eq;
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
    fn a_cycle_line_opens_with_the_local_time_and_the_padded_run_number() {
        let label = cycle_label(7, 5);
        let (time, number) = label.strip_prefix('[').and_then(|rest| rest.split_once("] ")).unwrap();

        assert_eq!(number, "#00007");
        assert!(NaiveTime::parse_from_str(time, "%H:%M:%S").is_ok(), "{label}");
    }

    #[test]
    fn a_large_run_total_widens_the_padding_to_fit() {
        assert_eq!(run_number_width(Some(RunCount::new(100_000))), 6);
        assert_eq!(run_number_width(Some(RunCount::new(1_000_000))), 7);
    }
}
