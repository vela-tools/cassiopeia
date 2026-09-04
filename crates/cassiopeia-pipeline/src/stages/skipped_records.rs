use crate::stages::coded_error::CodedError;
use cassiopeia_diagnostic::{code::diagnostic_code::DiagnosticCode, diagnostic_builder::from_error, severity::Severity};
use cassiopeia_reporter::reporter::DiagnosticSink;
use indexmap::IndexMap;
use std::{error::Error, hash::Hash, num::NonZeroU64};

/// What distinguishes one skipped-record reason from another.
///
/// The code alone is too coarse: two template failures on different attributes are different
/// problems, and the error itself cannot be the key, because these errors transitively wrap
/// `TemplateError`, `serde_json::Error`, `urn_rs::Error` and `io::Error`, none of which are `Hash`
/// or `Eq`. The rendered headline is what is both distinguishing and comparable.
#[derive(Debug, Eq, Hash, PartialEq)]
struct SkipReason<C> {
    /// Which reason this is.
    code: C,
    /// The failure as it renders.
    headline: Box<str>,
}

/// One distinct reason and how often it occurred.
struct SkippedGroup<E> {
    /// The first failure of this kind, kept whole so its `source()` chain survives to the report.
    first: E,
    /// How many records this reason skipped.
    occurrences: NonZeroU64,
}

/// The records one batch could not process, grouped by why.
///
/// The map is an [`IndexMap`] so a run over the same source reports its reasons in the same order
/// every time. There is no lock: all three stages that use this walk their batch's results
/// sequentially on one thread, so a shared map would be paying for concurrency that is not there.
pub(crate) struct SkippedRecords<C, E> {
    groups: IndexMap<SkipReason<C>, SkippedGroup<E>>,
}

impl<C, E> SkippedRecords<C, E>
where
    C: Copy + Eq + Hash + Into<DiagnosticCode>,
    E: CodedError<Code = C> + Error,
{
    /// Opens an empty sink.
    pub(crate) fn new() -> SkippedRecords<C, E> {
        SkippedRecords { groups: IndexMap::new() }
    }

    /// Records one skipped record, counting a repeat of a reason already seen.
    pub(crate) fn record(&mut self, error: E) {
        let reason = SkipReason {
            code: error.code(),
            headline: error.to_string().into_boxed_str(),
        };
        match self.groups.get_mut(&reason) {
            Some(group) => group.occurrences = group.occurrences.saturating_add(1),
            None => {
                self.groups.insert(
                    reason,
                    SkippedGroup {
                        first: error,
                        occurrences: NonZeroU64::MIN,
                    },
                );
            }
        }
    }

    /// How many records were skipped in total.
    pub(crate) fn total(&self) -> u64 {
        self.groups.values().fold(0, |total, group| total.saturating_add(group.occurrences.get()))
    }

    /// Whether nothing was skipped, which is the common case for a batch.
    pub(crate) fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Reports one diagnostic per distinct reason, each carrying its own occurrence count.
    ///
    /// The first failure is reported unrendered so the chain walk reaches whatever it wrapped, such
    /// as an `io::Error`'s operating-system reason or a template's own parse failure: detail a plain
    /// occurrence count alone would not carry.
    pub(crate) fn report(self, sink: &dyn DiagnosticSink, severity: Severity) {
        for (reason, group) in self.groups {
            sink.report(
                &from_error(severity, reason.code.into(), &group.first)
                    .with_occurrences(group.occurrences)
                    .build(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{stages::skipped_records::SkippedRecords, test_reporter::RecordingReporter};
    use cassiopeia_common::collection::CollectionName;
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, expander_code::ExpanderCode},
        severity::Severity,
    };
    use cassiopeia_expander::error::ExpanderError;
    use cassiopeia_reporter::backend::noop::NoopReporter;

    fn records() -> SkippedRecords<ExpanderCode, ExpanderError> {
        SkippedRecords::new()
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(records().is_empty());
        assert_eq!(records().total(), 0);
    }

    #[test]
    fn repeats_of_one_reason_are_counted_rather_than_listed() {
        let mut skipped = records();
        skipped.record(ExpanderError::CollectionMissing);
        skipped.record(ExpanderError::CollectionMissing);
        skipped.record(ExpanderError::UnmatchedCollection(CollectionName::from("Camera")));

        assert_eq!(skipped.total(), 3);
        assert_eq!(skipped.groups.len(), 2);
    }

    #[test]
    fn two_failures_of_one_code_but_different_text_are_distinct_reasons() {
        let mut skipped = records();
        skipped.record(ExpanderError::UnmatchedCollection(CollectionName::from("Camera")));
        skipped.record(ExpanderError::UnmatchedCollection(CollectionName::from("Flowcount")));

        assert_eq!(skipped.groups.len(), 2);
        assert!(
            skipped
                .groups
                .keys()
                .all(|reason| DiagnosticCode::from(reason.code) == DiagnosticCode::Expander(ExpanderCode::CollectionUnmatched))
        );
    }

    #[test]
    fn reporting_an_empty_sink_says_nothing() {
        records().report(&NoopReporter::new(), Severity::Warning);
    }

    #[test]
    fn one_reason_is_reported_once_however_many_records_it_skipped() {
        let sink = RecordingReporter::new();
        let mut skipped = records();
        for _ in 0..7 {
            skipped.record(ExpanderError::CollectionMissing);
        }

        skipped.report(&sink, Severity::Warning);

        let reported = sink.diagnostics();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].occurrences, 7);
        assert_eq!(reported[0].code, DiagnosticCode::Expander(ExpanderCode::CollectionMissing));
        assert!(reported[0].headline.contains("without a source collection"));
    }

    #[test]
    fn two_distinct_reasons_are_two_diagnostics() {
        let sink = RecordingReporter::new();
        let mut skipped = records();
        skipped.record(ExpanderError::CollectionMissing);
        skipped.record(ExpanderError::UnmatchedCollection(CollectionName::from("Camera")));
        skipped.record(ExpanderError::UnmatchedCollection(CollectionName::from("Camera")));

        skipped.report(&sink, Severity::Warning);

        let reported = sink.diagnostics();
        assert_eq!(reported.len(), 2);
        assert_eq!(reported[0].occurrences, 1);
        assert_eq!(reported[1].occurrences, 2);
        assert!(reported[1].headline.contains("Camera"));
    }
}
