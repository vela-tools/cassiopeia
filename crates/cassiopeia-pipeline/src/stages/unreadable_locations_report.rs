use cassiopeia_common::input::Input;
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ingestor::sensorthings::ingest_losses::{LossTally, UnreadableLocation};
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::num::NonZeroU64;

/// The one-line summary: the source, how many Things lost their location, and the first of them with
/// the encoding its Location names.
fn headline(source: &Input, first: &UnreadableLocation, total: NonZeroU64) -> String {
    match total.get() {
        1 => format!("In `{source}`, 1 Thing has a Location that is no GeoJSON, so its records have no `location`: {first}"),
        count => format!(
            "In `{source}`, {count} Things have a Location that is no GeoJSON, so their records have no `location`: {first}, and {} more",
            count.saturating_sub(1)
        ),
    }
}

/// Warns once for a SensorThings snapshot whose Things have Locations that are no `GeoJSON`, and
/// returns how many Things that was.
///
/// One diagnostic per snapshot, carrying every such Thing as an occurrence. `GeoJSON` is the only
/// Location encoding OGC 18-088 defines (section 8.2.2, Table 7), so another one cannot become a
/// `GeoProperty`. The snapshot is named by its `source`, so a downloaded one is named by its URL.
pub(crate) fn report_unreadable_locations(sink: &dyn DiagnosticSink, source: &Input, locations: &LossTally<UnreadableLocation>) -> u64 {
    let (Some(total), Some(first)) = (NonZeroU64::new(locations.total()), locations.named().first()) else {
        return 0;
    };
    sink.report(
        &DiagnosticBuilder::new(
            Severity::Warning,
            DiagnosticCode::Ingest(IngestCode::LocationsUnreadable),
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
    use crate::{stages::unreadable_locations_report::report_unreadable_locations, test_reporter::RecordingReporter};
    use cassiopeia_common::input::Input;
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode};
    use cassiopeia_ingestor::sensorthings::ingest_losses::{LocationEncoding, SensorThingsLosses, UnreadableLocation};
    use cassiopeia_sensorthings::entity_id::EntityId;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn losses(things: &[i64]) -> SensorThingsLosses {
        let mut losses = SensorThingsLosses::default();
        for &thing in things {
            losses.unreadable_location(UnreadableLocation {
                thing: EntityId::Integer(thing),
                encoding: LocationEncoding::Stated("text/plain".into()),
            });
        }
        losses
    }

    #[test]
    fn readable_locations_say_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(
            report_unreadable_locations(&sink, &Input::Local(PathBuf::from("/data/rivm.jsonl")), losses(&[]).unreadable_locations()),
            0
        );
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_unreadable_location_names_the_thing_and_its_encoding() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_locations(&sink, &Input::Local(PathBuf::from("/data/rivm.jsonl")), losses(&[12512]).unreadable_locations());

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::LocationsUnreadable));
        assert_eq!(reported[0].code.to_string(), "ingest-locations-unreadable");
        assert_eq!(
            reported[0].headline,
            "In `/data/rivm.jsonl`, 1 Thing has a Location that is no GeoJSON, so its records have no `location`: Thing 12512 encodes it as `text/plain`"
        );
        assert_eq!(reported[0].occurrences, 1);
    }

    #[test]
    fn several_unreadable_locations_name_the_first_and_count_every_thing() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_locations(
            &sink,
            &Input::Local(PathBuf::from("/data/rivm.jsonl")),
            losses(&[12512, 12513, 12514]).unreadable_locations(),
        );

        assert_eq!(total, 3);
        assert_eq!(
            sink.diagnostics()[0].headline,
            "In `/data/rivm.jsonl`, 3 Things have a Location that is no GeoJSON, so their records have no `location`: Thing 12512 encodes it as `text/plain`, and 2 more"
        );
        assert_eq!(sink.diagnostics()[0].occurrences, 3);
    }
}
