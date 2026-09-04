//! Middleware that collapses repeated diagnostics and tallies them for the run summary.

use crate::{
    guard::StageGuard,
    reporter::{DiagnosticSink, MessageLog, ProgressReporter, ProgressStage, Reporter, RunSummary, StageOutput, StageReporter},
    stage_id::StageId,
};
use cassiopeia_common::telemetry::run::TelemetrySnapshot;
use cassiopeia_diagnostic::{
    diagnostic::Diagnostic,
    reason::{Reason, Sighting},
    reason_tally::ReasonTally,
};
use execution_time::ExecutionTime;

/// A reporter wrapper that renders each distinct failure once and counts every repeat.
///
/// Collapsing keys on [`DiagnosticIdentity`](cassiopeia_diagnostic::diagnostic_identity::DiagnosticIdentity)
/// rather than on rendered text, so a headline that names a count cannot split a group and an entity
/// id cannot multiply one. The repeats are not lost: the same tally folds them by code into the run
/// summary's reason table, which is where a reader learns that one line stood for a hundred
/// failures.
///
/// Narrative log lines pass straight through: two stages can legitimately report the same step, and
/// it is the failures, not the narration, that flood a terminal.
pub struct DeduplicatingReporter {
    inner: Box<dyn Reporter>,
    tally: ReasonTally,
}

impl DeduplicatingReporter {
    /// Wraps `inner`, rendering each distinct diagnostic once.
    #[must_use]
    pub fn new(inner: Box<dyn Reporter>) -> DeduplicatingReporter {
        DeduplicatingReporter {
            inner,
            tally: ReasonTally::new(),
        }
    }
}

impl MessageLog for DeduplicatingReporter {
    fn info(&self, message: &str) {
        self.inner.info(message);
    }
    fn success(&self, message: &str) {
        self.inner.success(message);
    }
    fn debug(&self, message: &str) {
        self.inner.debug(message);
    }
    fn step(&self, current: usize, total: usize, message: &str) {
        self.inner.step(current, total, message);
    }
    fn raw_log(&self, message: &str) {
        self.inner.raw_log(message);
    }
}

impl DiagnosticSink for DeduplicatingReporter {
    fn report(&self, diagnostic: &Diagnostic) {
        match self.tally.record(diagnostic) {
            Sighting::First => self.inner.report(diagnostic),
            Sighting::Repeat => {}
        }
    }
}

impl RunSummary for DeduplicatingReporter {
    /// Appends the reasons this middleware collected to the ones it was handed, then draws the
    /// summary.
    ///
    /// Draining here is also the per-cycle reset: a scheduled run's next cycle starts with an empty
    /// tally, so a failure it repeats is rendered again rather than silently swallowed for the rest
    /// of the process.
    fn summary(&self, snapshot: &TelemetrySnapshot, reasons: &[Reason]) {
        let mut collected = reasons.to_vec();
        collected.extend(self.tally.drain());
        self.inner.summary(snapshot, &collected);
    }
}

impl ProgressReporter for DeduplicatingReporter {
    fn start_progress(&self, message: &str) {
        self.inner.start_progress(message);
    }
    fn update_progress(&self, message: &str) {
        self.inner.update_progress(message);
    }
    fn progress_set_length(&self, length: u64) {
        self.inner.progress_set_length(length);
    }
    fn progress_inc(&self) {
        self.inner.progress_inc();
    }
    fn stop_progress(&self) {
        self.inner.stop_progress();
    }
}

impl StageReporter for DeduplicatingReporter {
    fn set_quiet_stages(&self, output: StageOutput) {
        self.inner.set_quiet_stages(output);
    }
    fn enter_stage(&self, stage: Box<dyn ProgressStage>, execution_time: ExecutionTime) -> StageGuard {
        self.inner.enter_stage(stage, execution_time)
    }
    fn pre_register_stages(&self, stages: &[Box<dyn ProgressStage>]) {
        self.inner.pre_register_stages(stages);
    }
    fn stage_set_message(&self, id: StageId, message: &str) {
        self.inner.stage_set_message(id, message);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        guard::StageGuard,
        middleware::dedup::DeduplicatingReporter,
        reporter::{DiagnosticSink, MessageLog, ProgressReporter, ProgressStage, RunSummary, StageReporter},
    };
    use cassiopeia_common::telemetry::run::{RunTelemetry, TelemetrySnapshot};
    use cassiopeia_diagnostic::{
        code::{broker_code::BrokerCode, diagnostic_code::DiagnosticCode, schema_code::SchemaCode},
        context_field::ContextField,
        detail::Detail,
        diagnostic::Diagnostic,
        diagnostic_builder::DiagnosticBuilder,
        reason::Reason,
        severity::Severity,
    };
    use execution_time::ExecutionTime;
    use http::StatusCode;
    use iri_rs::IriBuf;
    use parking_lot::Mutex;
    use std::sync::Arc;

    /// Records the calls made to it into shared vectors observable after the reporter is boxed.
    struct RecordingReporter {
        infos: Arc<Mutex<Vec<String>>>,
        headlines: Arc<Mutex<Vec<String>>>,
        reasons: Arc<Mutex<Vec<Reason>>>,
    }

    impl MessageLog for RecordingReporter {
        fn info(&self, message: &str) {
            self.infos.lock().push(message.to_string());
        }
        fn success(&self, _message: &str) {}
        fn debug(&self, _message: &str) {}
        fn step(&self, _current: usize, _total: usize, _message: &str) {}
        fn raw_log(&self, _message: &str) {}
    }

    impl DiagnosticSink for RecordingReporter {
        fn report(&self, diagnostic: &Diagnostic) {
            self.headlines.lock().push(diagnostic.headline().to_string());
        }
    }

    impl RunSummary for RecordingReporter {
        fn summary(&self, _snapshot: &TelemetrySnapshot, reasons: &[Reason]) {
            self.reasons.lock().extend_from_slice(reasons);
        }
    }

    impl ProgressReporter for RecordingReporter {
        fn start_progress(&self, _message: &str) {}
        fn update_progress(&self, _message: &str) {}
        fn progress_set_length(&self, _length: u64) {}
        fn progress_inc(&self) {}
        fn stop_progress(&self) {}
    }

    impl StageReporter for RecordingReporter {
        fn enter_stage(&self, _stage: Box<dyn ProgressStage>, _execution_time: ExecutionTime) -> StageGuard {
            StageGuard::silent()
        }
        fn pre_register_stages(&self, _stages: &[Box<dyn ProgressStage>]) {}
    }

    /// A deduplicating reporter plus handles on everything its backend recorded.
    struct Harness {
        dedup: DeduplicatingReporter,
        infos: Arc<Mutex<Vec<String>>>,
        headlines: Arc<Mutex<Vec<String>>>,
        reasons: Arc<Mutex<Vec<Reason>>>,
    }

    fn harness() -> Harness {
        let infos = Arc::new(Mutex::new(Vec::new()));
        let headlines = Arc::new(Mutex::new(Vec::new()));
        let reasons = Arc::new(Mutex::new(Vec::new()));
        Harness {
            dedup: DeduplicatingReporter::new(Box::new(RecordingReporter {
                infos: Arc::clone(&infos),
                headlines: Arc::clone(&headlines),
                reasons: Arc::clone(&reasons),
            })),
            infos,
            headlines,
            reasons,
        }
    }

    /// One entity's rejection, differing from the next only in the entity it names.
    fn rejection(entity: &str, status: u16) -> Diagnostic {
        DiagnosticBuilder::new(
            Severity::Error,
            DiagnosticCode::Broker(BrokerCode::EntityRejected),
            format!("Broker rejected {entity}"),
        )
        .with_context(ContextField::HttpStatus(StatusCode::from_u16(status).unwrap()))
        .with_context(ContextField::Detail(Detail::new("attribute 'dateObserved' is not a valid DateTime")))
        .with_context(ContextField::Entities {
            first: IriBuf::new(entity.to_owned()).unwrap(),
            additional: 0,
        })
        .build()
    }

    fn snapshot(errors: u64) -> TelemetrySnapshot {
        let telemetry = RunTelemetry::new();
        telemetry.add_errors(errors);
        telemetry.snapshot()
    }

    #[test]
    fn a_repeated_diagnostic_reaches_the_backend_once() {
        let harness = harness();

        for _ in 0..3 {
            harness.dedup.report(&rejection("urn:ngsi-ld:A:1", 422));
        }

        assert_eq!(harness.headlines.lock().len(), 1);
    }

    #[test]
    fn a_hundred_rejections_differing_only_in_entity_render_once_and_count_each() {
        let harness = harness();

        for index in 0..100 {
            harness.dedup.report(&rejection(&format!("urn:ngsi-ld:A:{index}"), 422));
        }
        harness.dedup.summary(&snapshot(100), &[]);

        assert_eq!(harness.headlines.lock().len(), 1);
        assert_eq!(harness.reasons.lock()[0].count(), 100);
    }

    #[test]
    fn rejections_differing_in_a_defining_field_each_render() {
        let harness = harness();

        harness.dedup.report(&rejection("urn:ngsi-ld:A:1", 422));
        harness.dedup.report(&rejection("urn:ngsi-ld:A:2", 409));

        assert_eq!(harness.headlines.lock().len(), 2);
    }

    #[test]
    fn the_middleware_appends_its_reasons_to_the_ones_it_was_handed() {
        let harness = harness();
        let handed = vec![Reason::new(
            Severity::Warning,
            DiagnosticCode::Schema(SchemaCode::Absent),
            7,
            "no schema".into(),
        )];

        harness.dedup.report(&rejection("urn:ngsi-ld:A:1", 422));
        harness.dedup.summary(&snapshot(1), &handed);

        let reasons = harness.reasons.lock();
        assert_eq!(reasons.len(), 2);
        assert_eq!(reasons[0].code(), DiagnosticCode::Schema(SchemaCode::Absent));
        assert_eq!(reasons[1].code(), DiagnosticCode::Broker(BrokerCode::EntityRejected));
    }

    #[test]
    fn a_second_cycle_renders_the_same_failure_again() {
        let harness = harness();

        harness.dedup.report(&rejection("urn:ngsi-ld:A:1", 422));
        harness.dedup.summary(&snapshot(1), &[]);
        harness.dedup.report(&rejection("urn:ngsi-ld:A:1", 422));

        assert_eq!(harness.headlines.lock().len(), 2);
    }

    #[test]
    fn narrative_lines_are_no_longer_collapsed() {
        let harness = harness();

        harness.dedup.info("same");
        harness.dedup.info("same");

        assert_eq!(harness.infos.lock().len(), 2);
    }
}
