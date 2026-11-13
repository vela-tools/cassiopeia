use cassiopeia_collector::{collection_warnings::CollectionWarnings, sensorthings::range_empty::RangeEmpty};
use cassiopeia_common::{input::Input, masked_url::MaskedUrl};
use cassiopeia_diagnostic::{
    code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;
use cassiopeia_sensorthings::instant_text::instant_text;

/// The one-line summary: the server and the range, so a reader can see why nothing was read.
fn headline(range: &RangeEmpty) -> String {
    let window = range.window();
    format!(
        "In `{}`, the range from {} to {} holds no reading, because its start is not before its end, so this cycle requested no Observations",
        range.root(),
        instant_text(window.from()),
        instant_text(window.to())
    )
}

/// Names every range read whose range held no reading in one warning each, and returns how many
/// warnings that was.
///
/// Each warning names its server as the source, so an empty range of one server of a run with
/// several is told apart from the rest.
pub(crate) fn report_empty_ranges(sink: &dyn DiagnosticSink, warnings: &CollectionWarnings) -> u64 {
    warnings.empty_ranges().iter().fold(0, |reported, range| {
        sink.report(
            &DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Collect(CollectCode::RangeEmpty), headline(range))
                // The diagnostic owns its source, while the warnings keep the root they name.
                .with_context(ContextField::Source(Input::Remote(MaskedUrl::plain(range.root().url().clone()))))
                .build(),
        );
        reported + 1
    })
}

#[cfg(test)]
mod tests {
    use crate::{stages::empty_ranges_report::report_empty_ranges, test_reporter::RecordingReporter};
    use cassiopeia_collector::{collection_warnings::CollectionWarnings, sensorthings::range_empty::RangeEmpty};
    use cassiopeia_common::{input::Input, masked_url::MaskedUrl};
    use cassiopeia_diagnostic::{
        code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode},
        context_field::ContextField,
    };
    use cassiopeia_sensorthings::{read_window::ReadWindow, service_root::ServiceRoot};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use url::Url;

    #[test]
    fn a_collection_without_empty_ranges_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_empty_ranges(&sink, &CollectionWarnings::default()), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn an_empty_range_is_one_warning_naming_the_server_and_the_range() {
        let sink = RecordingReporter::new();
        let instant = |text: &str| -> DateTime<Utc> { DateTime::parse_from_rfc3339(text).unwrap().to_utc() };
        let root = Url::parse("https://h.example/v1.1").unwrap();
        let window = ReadWindow::new(instant("2026-11-13T00:00:00Z"), instant("2026-11-12T10:00:00Z")).unwrap_err();
        let mut warnings = CollectionWarnings::default();
        warnings.range_empty(RangeEmpty::new(ServiceRoot::try_from(root.clone()).unwrap(), window));

        let total = report_empty_ranges(&sink, &warnings);

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Collect(CollectCode::RangeEmpty));
        assert_eq!(reported[0].code.to_string(), "collect-range-empty");
        assert_eq!(
            reported[0].headline,
            "In `https://h.example/v1.1`, the range from 2026-11-13T00:00:00Z to 2026-11-12T10:00:00Z holds no reading, because its start is not before its end, so this cycle requested no Observations"
        );
        assert_eq!(reported[0].defining, [ContextField::Source(Input::Remote(MaskedUrl::plain(root)))]);
    }
}
