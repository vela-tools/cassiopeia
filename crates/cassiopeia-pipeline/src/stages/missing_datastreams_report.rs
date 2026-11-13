use cassiopeia_collector::{collection_warnings::CollectionWarnings, sensorthings::datastreams_missing::DatastreamsMissing};
use cassiopeia_common::{input::Input, masked_url::MaskedUrl};
use cassiopeia_diagnostic::{
    code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;
use cassiopeia_sensorthings::instant_text::instant_text;

/// The one-line summary: the server, the cursor, how many data streams are missing and since when,
/// the first few of them, and the rest counted.
fn headline(missing: &DatastreamsMissing) -> String {
    let count = missing.count().get();
    let streams = match count {
        1 => "1 data stream the cursor".to_owned(),
        count => format!("{count} data streams the cursor"),
    };
    let verb = if count == 1 { "is" } else { "are" };
    let named = missing.named().iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
    let ids = match count - missing.named().len() {
        0 => named,
        rest => format!("{named} and {rest} more"),
    };
    format!(
        "In `{}`, {streams} '{}' keeps a position for {verb} no longer listed, the earliest since {}: {ids}. The positions are kept, so a data stream listed again resumes where it stopped",
        missing.root(),
        missing.cursor(),
        instant_text(missing.since())
    )
}

/// Names the data streams every incremental read's server no longer lists in one warning per server,
/// and returns how many warnings that was.
///
/// Each warning names its server as the source, so the missing data streams of one server of a run
/// with several are told apart from the rest.
pub(crate) fn report_missing_datastreams(sink: &dyn DiagnosticSink, warnings: &CollectionWarnings) -> u64 {
    warnings.missing_datastreams().iter().fold(0, |reported, missing| {
        sink.report(
            &DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Collect(CollectCode::DatastreamsMissing), headline(missing))
                // The diagnostic owns its source, while the warnings keep the root they name.
                .with_context(ContextField::Source(Input::Remote(MaskedUrl::plain(missing.root().url().clone()))))
                .build(),
        );
        reported + 1
    })
}

#[cfg(test)]
mod tests {
    use crate::{stages::missing_datastreams_report::report_missing_datastreams, test_reporter::RecordingReporter};
    use cassiopeia_collector::{collection_warnings::CollectionWarnings, sensorthings::datastreams_missing::DatastreamsMissing};
    use cassiopeia_common::{input::Input, masked_url::MaskedUrl};
    use cassiopeia_diagnostic::{
        code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode},
        context_field::ContextField,
    };
    use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId, service_root::ServiceRoot};
    use cassiopeia_sensorthings_cursor::{cursor_update::CursorUpdate, positions::Positions, seen_observations::SeenObservations};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, time::Duration};
    use url::Url;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    /// The warnings of a cycle that started at 2026-11-12T10:00:00Z and listed none of the data
    /// streams `datastreams`, each written once at 08:00 in a cycle that started at 09:00.
    fn warnings_missing(datastreams: &[i64]) -> CollectionWarnings {
        let mut first = CursorUpdate::new(at("2026-11-12T09:00:00Z"), Duration::from_secs(3600));
        for datastream in datastreams {
            let written = SeenObservations::from(BTreeMap::from([(EntityId::Integer(*datastream), at("2026-11-12T08:00:00Z"))]));
            first.record(EntityId::Integer(*datastream), written);
        }
        let committed = first.apply(Positions::default());
        let cycle_start = at("2026-11-12T10:00:00Z");
        let missing = DatastreamsMissing::of(
            &ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap(),
            &CursorName::try_from("rivm-air".to_owned()).unwrap(),
            &committed,
            &CursorUpdate::new(cycle_start, Duration::from_secs(3600)),
            cycle_start,
        )
        .unwrap();
        let mut warnings = CollectionWarnings::default();
        warnings.datastreams_missing(missing);
        warnings
    }

    #[test]
    fn a_collection_without_missing_streams_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_missing_datastreams(&sink, &CollectionWarnings::default()), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn missing_streams_are_one_warning_naming_the_server_the_cursor_and_the_first_three() {
        let sink = RecordingReporter::new();

        let total = report_missing_datastreams(&sink, &warnings_missing(&[5, 7, 9, 11]));

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Collect(CollectCode::DatastreamsMissing));
        assert_eq!(reported[0].code.to_string(), "collect-datastreams-missing");
        assert_eq!(
            reported[0].headline,
            "In `https://h.example/v1.1`, 4 data streams the cursor 'rivm-air' keeps a position for are no longer listed, the earliest since 2026-11-12T10:00:00Z: 5, 7, 9 and 1 more. The positions are kept, so a data stream listed again resumes where it stopped"
        );
        assert_eq!(
            reported[0].defining,
            [ContextField::Source(Input::Remote(MaskedUrl::plain(
                Url::parse("https://h.example/v1.1").unwrap()
            )))]
        );
    }

    #[test]
    fn one_missing_stream_is_named_alone() {
        let sink = RecordingReporter::new();

        report_missing_datastreams(&sink, &warnings_missing(&[5]));

        assert_eq!(
            sink.diagnostics()[0].headline,
            "In `https://h.example/v1.1`, 1 data stream the cursor 'rivm-air' keeps a position for is no longer listed, the earliest since 2026-11-12T10:00:00Z: 5. The positions are kept, so a data stream listed again resumes where it stopped"
        );
    }
}
