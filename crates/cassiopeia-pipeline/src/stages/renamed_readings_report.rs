use crate::stages::source_warning::report_source_warning;
use cassiopeia_common::input::Input;
use cassiopeia_diagnostic::code::ingest_code::IngestCode;
use cassiopeia_ingestor::sensorthings::{ingest_losses::LossTally, reading_keys::RenamedReading};
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::num::NonZeroU64;

/// The one-line summary: the source, how many readings were renamed, the first few renames with the
/// key each is read by, and the rest counted.
///
/// The renames ride on the headline because the concise form shows nothing else, and a mapping
/// author needs the generated key to read the reading at all.
fn headline(source: &Input, renamed: &LossTally<RenamedReading>, total: NonZeroU64) -> String {
    let readings = match total.get() {
        1 => "1 reading is read by a generated key".to_owned(),
        count => format!("{count} readings are read by generated keys"),
    };
    let named = renamed.named().iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
    let details = match renamed.unnamed() {
        0 => named,
        rest => format!("{named}; and {rest} more"),
    };
    format!("In `{source}`, readings of one Thing observe the same property, so {readings}: {details}")
}

/// Names a SensorThings snapshot's renamed readings in one warning and returns how many readings
/// were renamed, of Datastreams and of `MultiDatastream` elements alike.
///
/// One diagnostic per snapshot, carrying every renamed reading as an occurrence, since the renames
/// share one cause: Things whose readings observe one property. The snapshot is named by
/// its `source`, so a downloaded one is named by its URL rather than by its temporary copy.
pub(crate) fn report_renamed_readings(sink: &dyn DiagnosticSink, source: &Input, renamed: &LossTally<RenamedReading>) -> u64 {
    let Some(total) = NonZeroU64::new(renamed.total()) else {
        return 0;
    };
    report_source_warning(sink, source, IngestCode::ReadingsRenamed, headline(source, renamed, total), total)
}

#[cfg(test)]
mod tests {
    use crate::{stages::renamed_readings_report::report_renamed_readings, test_reporter::RecordingReporter};
    use cassiopeia_common::input::Input;
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
        context_field::ContextField,
    };
    use cassiopeia_ingestor::sensorthings::{
        ingest_losses::SensorThingsLosses,
        reading_keys::{ReadingKey, ReadingSource, RenamedReading},
    };
    use cassiopeia_sensorthings::entity_id::EntityId;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    /// The losses of a snapshot whose data streams were renamed, each given as its Thing, its own id
    /// and its key.
    fn renamed(renames: &[(i64, i64, &str)]) -> SensorThingsLosses {
        let mut losses = SensorThingsLosses::default();
        for &(thing, datastream, key) in renames {
            losses.rename(RenamedReading {
                thing: EntityId::Integer(thing),
                source: ReadingSource::Datastream(EntityId::Integer(datastream)),
                key: ReadingKey::from(Box::<str>::from(key)),
            });
        }
        losses
    }

    #[test]
    fn a_snapshot_without_renames_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();

        let total = report_renamed_readings(
            &sink,
            &Input::Local(PathBuf::from("/data/rivm.jsonl")),
            SensorThingsLosses::default().renamed_readings(),
        );

        assert_eq!(total, 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_rename_is_one_warning_naming_the_thing_the_data_stream_and_its_key() {
        let sink = RecordingReporter::new();
        let losses = renamed(&[(12512, 9, "pm10@9")]);

        let total = report_renamed_readings(&sink, &Input::Local(PathBuf::from("/data/rivm.jsonl")), losses.renamed_readings());

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::ReadingsRenamed));
        assert_eq!(reported[0].code.to_string(), "ingest-readings-renamed");
        assert_eq!(
            reported[0].headline,
            "In `/data/rivm.jsonl`, readings of one Thing observe the same property, so 1 reading is read by a generated key: Thing 12512 reads Datastream 9 as `readings['pm10@9']`"
        );
        assert_eq!(reported[0].occurrences, 1);
        assert_eq!(reported[0].defining, [ContextField::Source(Input::Local(PathBuf::from("/data/rivm.jsonl")))]);
    }

    #[test]
    fn a_headline_names_three_renames_and_counts_the_rest() {
        let sink = RecordingReporter::new();
        let losses = renamed(&[(1, 9, "pm10@9"), (1, 10, "pm10@10"), (2, 11, "no2@11"), (3, 12, "no2@12"), (3, 13, "no2@13")]);

        let total = report_renamed_readings(&sink, &Input::Local(PathBuf::from("/data/rivm.jsonl")), losses.renamed_readings());

        assert_eq!(total, 5);
        assert_eq!(
            sink.diagnostics()[0].headline,
            "In `/data/rivm.jsonl`, readings of one Thing observe the same property, so 5 readings are read by generated keys: Thing 1 reads Datastream 9 as `readings['pm10@9']`; Thing 1 reads Datastream 10 as `readings['pm10@10']`; Thing 2 reads Datastream 11 as `readings['no2@11']`; and 2 more"
        );
        assert_eq!(sink.diagnostics()[0].occurrences, 5);
    }
}
