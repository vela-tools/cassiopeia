use cassiopeia_common::input::Input;
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::num::NonZeroU64;

/// Reports one warning about the whole of `source` under `code`, headed `headline`, that stands for
/// `occurrences` losses, and returns how many that is.
///
/// What an ingestor lost of one source is reported once per kind of loss, carrying every loss as an
/// occurrence, so a source that loses something on every line is one warning and its losses are
/// still counted. The source is named by its `Input`, so a downloaded one is named by its URL, the
/// same in every run, rather than by the temporary copy it was read from.
pub(crate) fn report_source_warning(sink: &dyn DiagnosticSink, source: &Input, code: IngestCode, headline: String, occurrences: NonZeroU64) -> u64 {
    sink.report(
        &DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Ingest(code), headline)
            // The diagnostic outlives the ingestor that names the source.
            .with_context(ContextField::Source(source.clone()))
            .with_occurrences(occurrences)
            .build(),
    );
    occurrences.get()
}

#[cfg(test)]
mod tests {
    use crate::{stages::source_warning::report_source_warning, test_reporter::RecordingReporter};
    use cassiopeia_common::input::Input;
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
        context_field::ContextField,
    };
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU64, path::PathBuf};

    #[test]
    fn one_warning_names_the_source_and_counts_every_loss() {
        let sink = RecordingReporter::new();
        let source = Input::Local(PathBuf::from("/data/rivm.jsonl"));

        let total = report_source_warning(
            &sink,
            &source,
            IngestCode::LocationsUnreadable,
            "Things lost their location".to_owned(),
            NonZeroU64::new(3).unwrap(),
        );

        let reported = sink.diagnostics();
        assert_eq!(total, 3);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::LocationsUnreadable));
        assert_eq!(reported[0].headline, "Things lost their location");
        assert_eq!(reported[0].occurrences, 3);
        assert_eq!(reported[0].defining, [ContextField::Source(source)]);
    }
}
