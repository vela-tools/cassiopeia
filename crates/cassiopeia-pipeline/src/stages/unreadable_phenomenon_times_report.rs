use cassiopeia_common::input::Input;
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ingestor::sensorthings::ingest_losses::LossTally;
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::num::NonZeroU64;

/// The one-line summary: the source, how many readings were dropped, and the first time text.
fn headline(source: &Input, first: &str, total: NonZeroU64) -> String {
    match total.get() {
        1 => format!("In `{source}`, 1 reading was dropped: its `phenomenonTime` `{first}` is neither an instant nor an interval of two instants"),
        count => format!(
            "In `{source}`, {count} readings were dropped: their `phenomenonTime` is neither an instant nor an interval of two instants, the first `{first}`"
        ),
    }
}

/// Warns once for a SensorThings snapshot whose Observations hold a `phenomenonTime` that names no
/// instant, and returns how many readings were dropped for it.
///
/// A reading's time becomes its attribute's `observedAt`, which is an instant (ETSI GS CIM 009
/// v1.9.1 clause 4.8), so a reading without one is dropped. One diagnostic per snapshot carries every
/// dropped reading as an occurrence. The snapshot is named by its `source`, so a downloaded one is
/// named by its URL.
pub(crate) fn report_unreadable_phenomenon_times(sink: &dyn DiagnosticSink, source: &Input, times: &LossTally<Box<str>>) -> u64 {
    let (Some(total), Some(first)) = (NonZeroU64::new(times.total()), times.named().first()) else {
        return 0;
    };
    sink.report(
        &DiagnosticBuilder::new(
            Severity::Warning,
            DiagnosticCode::Ingest(IngestCode::PhenomenonTimesUnreadable),
            headline(source, first, total),
        )
        // The diagnostic outlives the ingestor that names the source.
        .with_context(ContextField::Source(source.clone()))
        .with_occurrences(total)
        .build(),
    );
    total.get()
}

#[cfg(test)]
mod tests {
    use crate::{stages::unreadable_phenomenon_times_report::report_unreadable_phenomenon_times, test_reporter::RecordingReporter};
    use cassiopeia_common::input::Input;
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode};
    use cassiopeia_ingestor::sensorthings::ingest_losses::SensorThingsLosses;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn losses(texts: &[&str]) -> SensorThingsLosses {
        let mut losses = SensorThingsLosses::default();
        for text in texts {
            losses.unreadable_phenomenon_time((*text).into());
        }
        losses
    }

    #[test]
    fn readable_times_say_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(
            report_unreadable_phenomenon_times(
                &sink,
                &Input::Local(PathBuf::from("/data/rivm.jsonl")),
                losses(&[]).unreadable_phenomenon_times()
            ),
            0
        );
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_dropped_reading_quotes_its_time() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_phenomenon_times(
            &sink,
            &Input::Local(PathBuf::from("/data/rivm.jsonl")),
            losses(&["2026-11-12"]).unreadable_phenomenon_times(),
        );

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::PhenomenonTimesUnreadable));
        assert_eq!(reported[0].code.to_string(), "ingest-phenomenon-times-unreadable");
        assert_eq!(
            reported[0].headline,
            "In `/data/rivm.jsonl`, 1 reading was dropped: its `phenomenonTime` `2026-11-12` is neither an instant nor an interval of two instants"
        );
        assert_eq!(reported[0].occurrences, 1);
    }

    #[test]
    fn several_dropped_readings_quote_the_first_time_and_count_every_reading() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_phenomenon_times(
            &sink,
            &Input::Local(PathBuf::from("/data/rivm.jsonl")),
            losses(&["2026-11-12", "soon"]).unreadable_phenomenon_times(),
        );

        assert_eq!(total, 2);
        assert_eq!(
            sink.diagnostics()[0].headline,
            "In `/data/rivm.jsonl`, 2 readings were dropped: their `phenomenonTime` is neither an instant nor an interval of two instants, the first `2026-11-12`"
        );
    }
}
