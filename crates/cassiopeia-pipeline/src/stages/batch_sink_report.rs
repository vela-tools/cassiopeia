use cassiopeia_batch_sink::{batch_sink::BatchSink, recurring::Recurring};
use cassiopeia_diagnostic::diagnostic::Diagnostic;
use cassiopeia_reporter::reporter::DiagnosticSink;

/// Names every record of a batch sink once and returns how many occurrences they cover.
///
/// One diagnostic per record, built by `diagnostic` from its key and record, however many records
/// of the batch shared its failure: the sink already folded them into one, and the count it kept is
/// what the stage adds to its warnings.
pub(crate) fn report_sink<K, R: Recurring>(sink: &dyn DiagnosticSink, gathered: impl Into<BatchSink<K, R>>, diagnostic: impl Fn(K, &R) -> Diagnostic) -> u64 {
    gathered.into().into_entries().fold(0, |total: u64, (key, record)| {
        sink.report(&diagnostic(key, &record));
        total.saturating_add(record.occurrences().get())
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        stages::{batch_sink_report::report_sink, unreadable_timestamp_report::unreadable_timestamp_diagnostic},
        test_reporter::RecordingReporter,
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use cassiopeia_unreadable_timestamps::{timestamp_loss::TimestampLoss, unreadable_timestamps::UnreadableTimestamps};
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn an_empty_sink_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_sink(&sink, UnreadableTimestamps::new(), unreadable_timestamp_diagnostic), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_diagnostic_is_published_per_record_and_the_total_sums_their_occurrences() {
        let sink = RecordingReporter::new();
        let unreadable = UnreadableTimestamps::new();
        unreadable.record(TimestampLoss::ObservedAt, &name("temperature"), "one");
        unreadable.record(TimestampLoss::ObservedAt, &name("temperature"), "two");
        unreadable.record(TimestampLoss::ExpiresAt, &name("humidity"), "three");

        let total = report_sink(&sink, unreadable, unreadable_timestamp_diagnostic);

        let reported = sink.diagnostics();
        assert_eq!(total, 3);
        assert_eq!(reported.len(), 2);
        assert_eq!(reported[0].occurrences, 2);
        assert_eq!(reported[1].occurrences, 1);
    }
}
