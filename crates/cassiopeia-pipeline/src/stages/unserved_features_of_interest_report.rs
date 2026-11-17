use cassiopeia_collector::{collection_warnings::CollectionWarnings, sensorthings::features_of_interest_unserved::FeaturesOfInterestUnserved};
use cassiopeia_common::{input::Input, masked_url::MaskedUrl};
use cassiopeia_diagnostic::{
    code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;

/// The one-line summary: the server, how many Observations it served without their
/// `FeatureOfInterest`, and the first of them with its data stream.
fn headline(unserved: &FeaturesOfInterestUnserved) -> String {
    let root = unserved.root();
    let first = unserved.first();
    match unserved.count().get() {
        1 => format!(
            "`{root}` served 1 Observation without the FeatureOfInterest that `featuresOfInterest` asks for, so its readings have no `featureOfInterest`: {first}"
        ),
        count => format!(
            "`{root}` served {count} Observations without the FeatureOfInterest that `featuresOfInterest` asks for, so their readings have no `featureOfInterest`: {first}, and {} more",
            count.saturating_sub(1)
        ),
    }
}

/// Names every server that served a read Observations without the `FeatureOfInterest` it asked for
/// in one warning each, and returns how many warnings that was.
///
/// Each warning names its server as the source, so the servers of a run with several are told apart.
pub(crate) fn report_unserved_features_of_interest(sink: &dyn DiagnosticSink, warnings: &CollectionWarnings) -> u64 {
    warnings.unserved_features_of_interest().iter().fold(0, |reported, unserved| {
        sink.report(
            &DiagnosticBuilder::new(
                Severity::Warning,
                DiagnosticCode::Collect(CollectCode::FeaturesOfInterestUnserved),
                headline(unserved),
            )
            // The diagnostic owns its source, while the warnings keep the root they name.
            .with_context(ContextField::Source(Input::Remote(MaskedUrl::plain(unserved.root().url().clone()))))
            .build(),
        );
        reported + 1
    })
}

#[cfg(test)]
mod tests {
    use crate::{stages::unserved_features_of_interest_report::report_unserved_features_of_interest, test_reporter::RecordingReporter};
    use cassiopeia_collector::{
        collection_warnings::CollectionWarnings,
        sensorthings::features_of_interest_unserved::{FeaturesOfInterestUnserved, UnservedObservation},
    };
    use cassiopeia_common::{input::Input, masked_url::MaskedUrl};
    use cassiopeia_diagnostic::{
        code::{collect_code::CollectCode, diagnostic_code::DiagnosticCode},
        context_field::ContextField,
    };
    use cassiopeia_sensorthings::{entity_id::EntityId, observation_name::ObservationName, service_root::ServiceRoot, stream_id::StreamId};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU64;
    use url::Url;

    /// The warnings of a read of `https://h.example/v1.1` that wrote `count` Observations without
    /// their `FeatureOfInterest`, the first Observation 70 of Datastream 7.
    fn unserved(count: u64) -> CollectionWarnings {
        let mut warnings = CollectionWarnings::default();
        warnings.features_of_interest_unserved(FeaturesOfInterestUnserved::new(
            ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap(),
            UnservedObservation {
                observation: ObservationName::Id(EntityId::Integer(70)),
                datastream: StreamId::Datastream(EntityId::Integer(7)),
            },
            NonZeroU64::new(count).unwrap(),
        ));
        warnings
    }

    #[test]
    fn a_collection_whose_observations_carry_their_features_of_interest_says_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_unserved_features_of_interest(&sink, &CollectionWarnings::default()), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn unserved_features_of_interest_are_one_warning_naming_the_server_and_the_first_observation() {
        let sink = RecordingReporter::new();

        let total = report_unserved_features_of_interest(&sink, &unserved(3));

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Collect(CollectCode::FeaturesOfInterestUnserved));
        assert_eq!(reported[0].code.to_string(), "collect-features-of-interest-unserved");
        assert_eq!(
            reported[0].headline,
            "`https://h.example/v1.1` served 3 Observations without the FeatureOfInterest that `featuresOfInterest` asks for, so their readings have no `featureOfInterest`: Observation 70 of Datastream 7, and 2 more"
        );
        assert_eq!(
            reported[0].defining,
            [ContextField::Source(Input::Remote(MaskedUrl::plain(
                Url::parse("https://h.example/v1.1").unwrap()
            )))]
        );
    }

    #[test]
    fn one_unserved_feature_of_interest_is_named_alone() {
        let sink = RecordingReporter::new();

        report_unserved_features_of_interest(&sink, &unserved(1));

        assert_eq!(
            sink.diagnostics()[0].headline,
            "`https://h.example/v1.1` served 1 Observation without the FeatureOfInterest that `featuresOfInterest` asks for, so its readings have no `featureOfInterest`: Observation 70 of Datastream 7"
        );
    }
}
