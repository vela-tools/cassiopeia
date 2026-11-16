use crate::stages::source_warning::report_source_warning;
use cassiopeia_common::input::Input;
use cassiopeia_diagnostic::code::ingest_code::IngestCode;
use cassiopeia_ingestor::sensorthings::ingest_losses::{LossTally, UnreadableFeatureOfInterest};
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::num::NonZeroU64;

/// The one-line summary: the source, how many readings lost the geometry of their
/// `FeatureOfInterest`, and the first such `FeatureOfInterest` with the encoding it names.
fn headline(source: &Input, first: &UnreadableFeatureOfInterest, total: NonZeroU64) -> String {
    match total.get() {
        1 => format!("In `{source}`, 1 reading has a FeatureOfInterest that is no GeoJSON, so its `featureOfInterest` has no `feature`: {first}"),
        count => format!(
            "In `{source}`, {count} readings have a FeatureOfInterest that is no GeoJSON, so their `featureOfInterest` has no `feature`: {first}, and {} more",
            count.saturating_sub(1)
        ),
    }
}

/// Warns once for a SensorThings snapshot whose readings have a `FeatureOfInterest` that is no
/// `GeoJSON`, and returns how many readings that was.
///
/// One diagnostic per snapshot, carrying every such reading as an occurrence. `GeoJSON` is the only
/// `FeatureOfInterest` encoding OGC 18-088 defines (section 8.2.8), so another one cannot become a
/// `GeoProperty`. The snapshot is named by its `source`, so a downloaded one is named by its URL.
pub(crate) fn report_unreadable_features_of_interest(sink: &dyn DiagnosticSink, source: &Input, features: &LossTally<UnreadableFeatureOfInterest>) -> u64 {
    let (Some(total), Some(first)) = (NonZeroU64::new(features.total()), features.named().first()) else {
        return 0;
    };
    report_source_warning(sink, source, IngestCode::FeaturesOfInterestUnreadable, headline(source, first, total), total)
}

#[cfg(test)]
mod tests {
    use crate::{stages::unreadable_features_of_interest_report::report_unreadable_features_of_interest, test_reporter::RecordingReporter};
    use cassiopeia_common::input::Input;
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
        context_field::ContextField,
    };
    use cassiopeia_ingestor::sensorthings::ingest_losses::{FeatureOfInterestName, GeometryEncoding, SensorThingsLosses, UnreadableFeatureOfInterest};
    use cassiopeia_sensorthings::entity_id::EntityId;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    /// The losses of a snapshot whose readings each had one of the PDF features of interest `features`.
    fn losses(features: &[i64]) -> SensorThingsLosses {
        let mut losses = SensorThingsLosses::default();
        for &feature in features {
            losses.unreadable_feature_of_interest(UnreadableFeatureOfInterest {
                feature: FeatureOfInterestName::Id(EntityId::Integer(feature)),
                encoding: GeometryEncoding::Stated("application/pdf".into()),
            });
        }
        losses
    }

    fn source() -> Input {
        Input::Local(PathBuf::from("/data/bus.jsonl"))
    }

    #[test]
    fn readable_features_of_interest_say_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(
            report_unreadable_features_of_interest(&sink, &source(), losses(&[]).unreadable_features_of_interest()),
            0
        );
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_unreadable_feature_of_interest_names_it_and_its_encoding() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_features_of_interest(&sink, &source(), losses(&[4711]).unreadable_features_of_interest());

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::FeaturesOfInterestUnreadable));
        assert_eq!(reported[0].code.to_string(), "ingest-features-of-interest-unreadable");
        assert_eq!(
            reported[0].headline,
            "In `/data/bus.jsonl`, 1 reading has a FeatureOfInterest that is no GeoJSON, so its `featureOfInterest` has no `feature`: FeatureOfInterest 4711 encodes it as `application/pdf`"
        );
        assert_eq!(reported[0].occurrences, 1);
        assert_eq!(reported[0].defining, [ContextField::Source(source())]);
    }

    #[test]
    fn several_unreadable_features_of_interest_name_the_first_and_count_every_reading() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_features_of_interest(&sink, &source(), losses(&[4711, 4711, 4712]).unreadable_features_of_interest());

        assert_eq!(total, 3);
        assert_eq!(
            sink.diagnostics()[0].headline,
            "In `/data/bus.jsonl`, 3 readings have a FeatureOfInterest that is no GeoJSON, so their `featureOfInterest` has no `feature`: FeatureOfInterest 4711 encodes it as `application/pdf`, and 2 more"
        );
        assert_eq!(sink.diagnostics()[0].occurrences, 3);
    }
}
