use crate::{
    error::PipelineError,
    stages::{
        nonconformant_types::{NonconformantTypes, WarnedEntity},
        pump::BatchSender,
        stage_env::StageEnv,
        validation_abort::abort_error,
        validation_decision::{Decision, Reporting, decide, diagnostics_level},
        validation_report::SharedReport,
    },
};
use cassiopeia_common::{batch::Batch, signal::Signal, telemetry::run::RunTelemetry};
use cassiopeia_manifest::output::validation_mode::ValidationMode;
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_reporter::{guard::StageGuard, reporter::Reporter};
use cassiopeia_validator::{
    error::Result as ValidatorResult,
    schema_verdict::{DiagnosticsLevel, ValidationDiagnostics, ValidationOutcome},
};
use std::{mem, ops::ControlFlow, sync::Arc};

/// How a stage that renders verdicts enforces them: how strictly, and the report its failures are
/// recorded in.
pub(crate) struct ValidationSettings {
    /// How strictly a verdict is enforced before an entity is written.
    pub(crate) mode: ValidationMode,
    /// The report failures are recorded in, when the run asked for one.
    pub(crate) report: Option<SharedReport>,
}

/// What enforcing one verdict decided for its entity.
enum Enforced {
    /// Forward the entity and count it as processed.
    Passed,
    /// Forward the entity but count it only as a warning, not as processed. The warning counts in
    /// the run's totals only when it is the first of its entity.
    Warned(WarnedEntity),
    /// Tear the run down with this error; the entity is not forwarded.
    Aborted(PipelineError),
}

/// Applies the run's [`ValidationMode`] to the verdicts a stage renders: in the relaxed mode a
/// failure only warns and the entity is still written; in the fail-fast modes a schema-backed failure
/// (and, under the strict mode, a missing schema) emits an error signal that tears the pipeline down.
/// Failures are optionally recorded in the run's report.
///
/// Warnings are grouped by (reason, entity type) across the stage's whole stream and reported once at
/// the end, so a stream in which every entity is nonconformant costs one line per type rather than one
/// per entity.
pub(crate) struct Enforcement {
    /// How strictly a verdict is enforced.
    mode: ValidationMode,
    /// How much work a nonconformant result needs for this run's policy and output.
    diagnostics_level: DiagnosticsLevel,
    /// The report failures are recorded in, present only when the run asked for one.
    report: Option<SharedReport>,
    /// The entity types warned about, grouped by why, flushed when the stream ends.
    warned_types: NonconformantTypes,
    /// The reporter warnings and the final report-write outcome are logged to.
    reporter: &'static dyn Reporter,
    /// The run telemetry an aborting or warning verdict is counted into, so the run summary's error
    /// and warning totals match the exit status.
    telemetry: Arc<RunTelemetry>,
}

impl Enforcement {
    /// Enforces verdicts as `settings` says, reporting through `env`.
    pub(crate) fn new(settings: ValidationSettings, env: &StageEnv) -> Enforcement {
        let ValidationSettings { mode, report } = settings;
        let reporting = if report.is_some() { Reporting::Enabled } else { Reporting::Disabled };
        Enforcement {
            mode,
            diagnostics_level: diagnostics_level(mode, reporting),
            report,
            warned_types: NonconformantTypes::new(),
            reporter: env.reporter,
            telemetry: Arc::clone(&env.telemetry),
        }
    }

    /// The cheapest diagnostics a verdict needs for this run's policy and report.
    pub(crate) const fn diagnostics_level(&self) -> DiagnosticsLevel {
        self.diagnostics_level
    }

    /// Counts `count` entities as checked, in the report.
    pub(crate) fn observe(&self, count: u64) {
        if let Some(report) = &self.report {
            report.observe(count);
        }
    }

    /// Chooses what happens to `entity` from its verdict, and records what the run reports about it.
    fn enforce(&mut self, entity: &NgsiLdEntity, verdict: ValidatorResult<ValidationOutcome>) -> Enforced {
        match decide(self.mode, &verdict) {
            Decision::Forward => Enforced::Passed,
            Decision::Warn(reason) => {
                let warned = self.warned_types.record(reason, entity);
                if let (Some(report), Ok(outcome)) = (&self.report, verdict)
                    && let ValidationDiagnostics::Report { entry, .. } = outcome.into_diagnostics()
                {
                    report.record(entry);
                }
                Enforced::Warned(warned)
            }
            Decision::Abort => Enforced::Aborted(abort_error(verdict, entity, self.report.as_ref())),
        }
    }

    /// Reports the warned types and releases the stage's hold on the report, once its stream ended.
    pub(crate) fn finish(&mut self) {
        let warned = mem::replace(&mut self.warned_types, NonconformantTypes::new());
        if !warned.is_empty() {
            warned.report(self.reporter);
        }
        if let Some(report) = self.report.take() {
            report.release(self.reporter);
        }
    }
}

/// One batch's entities as their verdicts are enforced: what is forwarded, how it is counted, and
/// the error that stopped the batch, if one did.
///
/// The counts are distinct on purpose: a passing entity advances the progress bar, a warned one
/// is completed without advancing it (its warning already showed there), and both are forwarded. A
/// series run checks each record of an entity on its own, so only the first warned record of an
/// entity adds a warning, and the run's warnings count the entities it writes.
pub(crate) struct EnforcedBatch<T> {
    /// What is forwarded, in arrival order.
    forwarded: Batch<T>,
    /// Entities that conformed and advance the stage's progress.
    passed: u64,
    /// Entities forwarded despite a warning, counted as completed but not as processed.
    warned: u64,
    /// The warned entities not warned about before, each one warning in the run's totals.
    warnings: u64,
    /// The error that stopped the batch.
    abort: Option<PipelineError>,
}

impl<T: Send + 'static> EnforcedBatch<T> {
    /// An empty batch with room for `capacity` items.
    pub(crate) fn with_capacity(capacity: usize) -> EnforcedBatch<T> {
        EnforcedBatch {
            forwarded: Batch::with_capacity(capacity),
            passed: 0,
            warned: 0,
            warnings: 0,
            abort: None,
        }
    }

    /// Enforces `verdict` on `entity`, forwarding what `forward` makes of it unless the verdict
    /// aborts the run, which breaks the batch: the entities admitted before it are still forwarded,
    /// and the rest of the batch is dropped. Fail-fast is at batch granularity.
    pub(crate) fn admit(
        &mut self,
        enforcement: &mut Enforcement,
        entity: NgsiLdEntity,
        verdict: ValidatorResult<ValidationOutcome>,
        forward: impl FnOnce(NgsiLdEntity) -> T,
    ) -> ControlFlow<()> {
        match enforcement.enforce(&entity, verdict) {
            Enforced::Passed => {
                self.passed += 1;
                self.forwarded.push(forward(entity));
                ControlFlow::Continue(())
            }
            Enforced::Warned(warned) => {
                self.warned += 1;
                match warned {
                    WarnedEntity::New => self.warnings += 1,
                    WarnedEntity::Repeated => {}
                }
                self.forwarded.push(forward(entity));
                ControlFlow::Continue(())
            }
            Enforced::Aborted(error) => {
                self.abort = Some(error);
                ControlFlow::Break(())
            }
        }
    }

    /// Records the batch's tally and hands what it forwards downstream, then the error that stopped
    /// it, if one did.
    pub(crate) fn send(self, enforcement: &Enforcement, stage: &StageGuard, tx: &BatchSender<T>) -> ControlFlow<()> {
        let EnforcedBatch {
            forwarded,
            passed,
            warned,
            warnings,
            abort,
        } = self;
        if warnings > 0 {
            stage.warn_inc_by(warnings);
            enforcement.telemetry.add_warnings(warnings);
        }
        let sent = if forwarded.is_empty() {
            ControlFlow::Continue(())
        } else if stage.measure_output_wait(|| tx.send(Signal::Data(forwarded)).is_ok()) {
            stage.inc_by(passed);
            // A warned entity is still forwarded, so it completes the stage's work, but it is not
            // counted as processed on the bar (`warn_inc_by` already reflected it there).
            stage.complete(warned);
            ControlFlow::Continue(())
        } else {
            stage.fail(passed.saturating_add(warned));
            ControlFlow::Break(())
        };

        match abort {
            Some(error) => {
                stage.fail(1);
                enforcement.telemetry.add_errors(1);
                let _ = tx.send(Signal::Error(error));
                ControlFlow::Break(())
            }
            None => sent,
        }
    }
}
