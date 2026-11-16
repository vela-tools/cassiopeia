use crate::stages::source_warning::report_source_warning;
use cassiopeia_common::input::Input;
use cassiopeia_diagnostic::code::ingest_code::IngestCode;
use cassiopeia_ingestor::sensorthings::ingest_losses::{LossTally, UnreadableHistoricalLocation};
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::num::NonZeroU64;

/// The one-line summary: the source, how many `HistoricalLocations` gave no record, and the first of
/// them with its Thing and what could not be read of it.
fn headline(source: &Input, first: &UnreadableHistoricalLocation, total: NonZeroU64) -> String {
    match total.get() {
        1 => format!("In `{source}`, 1 HistoricalLocation gives no record: {first}"),
        count => format!(
            "In `{source}`, {count} HistoricalLocations give no record: {first}, and {} more",
            count.saturating_sub(1)
        ),
    }
}

/// Warns once for a SensorThings snapshot whose `HistoricalLocations` have a `time` that names no
/// instant, or no Location in `GeoJSON`, and returns how many `HistoricalLocations` that was.
///
/// One diagnostic per snapshot, carrying every such `HistoricalLocation` as an occurrence. Neither
/// gives a place at a time a Thing's track could hold, so neither gives a record. The snapshot is
/// named by its `source`, so a downloaded one is named by its URL.
pub(crate) fn report_unreadable_historical_locations(
    sink: &dyn DiagnosticSink,
    source: &Input,
    historical_locations: &LossTally<UnreadableHistoricalLocation>,
) -> u64 {
    let (Some(total), Some(first)) = (NonZeroU64::new(historical_locations.total()), historical_locations.named().first()) else {
        return 0;
    };
    report_source_warning(sink, source, IngestCode::HistoricalLocationsUnreadable, headline(source, first, total), total)
}

#[cfg(test)]
mod tests {
    use crate::{stages::unreadable_historical_locations_report::report_unreadable_historical_locations, test_reporter::RecordingReporter};
    use cassiopeia_common::input::Input;
    use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode};
    use cassiopeia_ingestor::sensorthings::ingest_losses::{HistoricalLocationFault, HistoricalLocationName, SensorThingsLosses, UnreadableHistoricalLocation};
    use cassiopeia_sensorthings::entity_id::EntityId;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn losses(historical_locations: &[i64]) -> SensorThingsLosses {
        let mut losses = SensorThingsLosses::default();
        for &historical_location in historical_locations {
            losses.unreadable_historical_location(UnreadableHistoricalLocation {
                historical_location: HistoricalLocationName::Id(EntityId::Integer(historical_location)),
                thing: EntityId::Integer(12512),
                fault: HistoricalLocationFault::Time("2026-11-12".into()),
            });
        }
        losses
    }

    fn source() -> Input {
        Input::Local(PathBuf::from("/data/bus.jsonl"))
    }

    #[test]
    fn readable_historical_locations_say_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(
            report_unreadable_historical_locations(&sink, &source(), losses(&[]).unreadable_historical_locations()),
            0
        );
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_unreadable_historical_location_names_it_its_thing_and_its_fault() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_historical_locations(&sink, &source(), losses(&[88]).unreadable_historical_locations());

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::HistoricalLocationsUnreadable));
        assert_eq!(reported[0].code.to_string(), "ingest-historical-locations-unreadable");
        assert_eq!(
            reported[0].headline,
            "In `/data/bus.jsonl`, 1 HistoricalLocation gives no record: HistoricalLocation 88 of Thing 12512 has the `time` `2026-11-12`, which names no instant"
        );
        assert_eq!(reported[0].occurrences, 1);
    }

    #[test]
    fn several_unreadable_historical_locations_name_the_first_and_count_every_one() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_historical_locations(&sink, &source(), losses(&[88, 89]).unreadable_historical_locations());

        assert_eq!(total, 2);
        assert_eq!(
            sink.diagnostics()[0].headline,
            "In `/data/bus.jsonl`, 2 HistoricalLocations give no record: HistoricalLocation 88 of Thing 12512 has the `time` `2026-11-12`, which names no instant, and 1 more"
        );
        assert_eq!(sink.diagnostics()[0].occurrences, 2);
    }
}
