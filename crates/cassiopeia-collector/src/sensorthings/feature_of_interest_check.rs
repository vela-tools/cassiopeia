use crate::{
    collection_warnings::CollectionWarnings,
    sensorthings::features_of_interest_unserved::{FeaturesOfInterestUnserved, UnservedObservation},
};
use cassiopeia_sensorthings::{
    entity_id::EntityId,
    feature_of_interest_members::FEATURE_OF_INTEREST_MEMBER,
    observation_name::ObservationName,
    service::SensorThingsService,
    service_root::ServiceRoot,
    settings::features_of_interest::FeaturesOfInterest,
    stream_id::StreamId,
    stream_kind::StreamKind,
};
use serde_json::{Map, Value};
use std::num::NonZeroU64;

/// Counts the Observations a read writes without the `FeatureOfInterest` it asked for, and names the
/// first, so the cycle warns once for its server.
///
/// A read that did not ask for features of interest counts none. An Observation whose
/// `FeatureOfInterest` member is absent, `null` or no object is counted, as its readings get no
/// `featureOfInterest` from it.
pub(crate) struct FeatureOfInterestCheck<'a> {
    /// The service root the read was built on, which the warning names.
    root: &'a ServiceRoot,
    /// Whether the read asked for features of interest.
    features: FeaturesOfInterest,
    /// The first Observation written without its `FeatureOfInterest`, and how many were.
    unserved: Option<(UnservedObservation, NonZeroU64)>,
}

impl<'a> FeatureOfInterestCheck<'a> {
    /// The check of a read of `service`, before it has counted anything.
    pub(crate) fn new(service: &'a SensorThingsService) -> FeatureOfInterestCheck<'a> {
        FeatureOfInterestCheck {
            root: service.root(),
            features: service.settings().features_of_interest(),
            unserved: None,
        }
    }

    /// Counts `observation`, written for the data stream `datastream`, when the read asked for its
    /// `FeatureOfInterest` and the server served it without one.
    pub(crate) fn observation(&mut self, datastream: &StreamId, observation: &Map<String, Value>) {
        if self.is_unserved(observation) {
            self.count(datastream, observation);
        }
    }

    /// Counts the newest Observation of `datastream`, a data stream of kind `kind` a latest discovery
    /// served, as [`FeatureOfInterestCheck::observation`] counts an Observation.
    ///
    /// The newest Observation is the first of the data stream's `Observations`, as the ingestor reads
    /// it. A data stream without one holds nothing to count, and one without a readable id is not
    /// counted, because the ingestor fails its line.
    pub(crate) fn newest_observation(&mut self, kind: StreamKind, datastream: &Map<String, Value>) {
        let Some(newest) = newest(datastream) else {
            return;
        };
        if self.is_unserved(newest)
            && let Some(id) = StreamId::of(kind, datastream)
        {
            self.count(&id, newest);
        }
    }

    /// Records the Observations the check counted, if it counted any, in `warnings`.
    pub(crate) fn record(self, warnings: &mut CollectionWarnings) {
        if let Some((first, count)) = self.unserved {
            // The warning names the root, while the service keeps its own.
            warnings.features_of_interest_unserved(FeaturesOfInterestUnserved::new(self.root.clone(), first, count));
        }
    }

    /// Whether the read asked for features of interest and `observation` was served without its
    /// `FeatureOfInterest`.
    fn is_unserved(&self, observation: &Map<String, Value>) -> bool {
        match self.features {
            FeaturesOfInterest::Requested => !matches!(observation.get(FEATURE_OF_INTEREST_MEMBER), Some(Value::Object(_))),
            FeaturesOfInterest::Omitted => false,
        }
    }

    /// Counts `observation` of `datastream`, and names it when it is the first.
    fn count(&mut self, datastream: &StreamId, observation: &Map<String, Value>) {
        if let Some((_, count)) = &mut self.unserved {
            *count = count.saturating_add(1);
            return;
        }
        let first = UnservedObservation {
            observation: EntityId::of(observation).map_or(ObservationName::WithoutId, ObservationName::Id),
            // The warning names the data stream, while the read keeps it for its lines.
            datastream: datastream.clone(),
        };
        self.unserved = Some((first, NonZeroU64::MIN));
    }
}

/// The newest Observation of `datastream`, a data stream a latest discovery served: the first of its
/// `Observations`, when that is an object.
fn newest(datastream: &Map<String, Value>) -> Option<&Map<String, Value>> {
    datastream.get("Observations")?.as_array()?.first()?.as_object()
}

#[cfg(test)]
mod tests {
    use crate::{
        collection_warnings::CollectionWarnings,
        sensorthings::{
            feature_of_interest_check::FeatureOfInterestCheck,
            features_of_interest_unserved::{FeaturesOfInterestUnserved, UnservedObservation},
        },
    };
    use cassiopeia_sensorthings::{
        entity_id::EntityId,
        observation_name::ObservationName,
        service::SensorThingsService,
        service_root::ServiceRoot,
        stream_id::StreamId,
        stream_kind::StreamKind,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};
    use std::num::NonZeroU64;
    use url::Url;

    fn root() -> ServiceRoot {
        ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap()
    }

    /// A read of `https://h.example/v1.1` with the settings `settings`.
    fn service(settings: &str) -> SensorThingsService {
        SensorThingsService::new(root(), serde_json::from_str(settings).unwrap())
    }

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    /// The Datastream `id`.
    fn datastream(id: i64) -> StreamId {
        StreamId::Datastream(EntityId::Integer(id))
    }

    /// What `check` recorded.
    fn recorded(check: FeatureOfInterestCheck<'_>) -> CollectionWarnings {
        let mut warnings = CollectionWarnings::default();
        check.record(&mut warnings);
        warnings
    }

    #[test]
    fn observations_without_their_feature_of_interest_are_counted_and_the_first_is_named() {
        let service = service(r#"{"featuresOfInterest": true}"#);
        let mut check = FeatureOfInterestCheck::new(&service);

        check.observation(&datastream(7), &object(json!({"@iot.id": 70, "result": 1.0})));
        check.observation(&datastream(7), &object(json!({"@iot.id": 71, "result": 2.0, "FeatureOfInterest": null})));
        check.observation(&datastream(8), &object(json!({"@iot.id": 80, "result": 3.0, "FeatureOfInterest": "stop 12"})));

        assert_eq!(
            recorded(check).unserved_features_of_interest(),
            [FeaturesOfInterestUnserved::new(
                root(),
                UnservedObservation {
                    observation: ObservationName::Id(EntityId::Integer(70)),
                    datastream: datastream(7),
                },
                NonZeroU64::new(3).unwrap(),
            )]
        );
    }

    #[test]
    fn observations_served_with_their_feature_of_interest_record_nothing() {
        let service = service(r#"{"featuresOfInterest": true}"#);
        let mut check = FeatureOfInterestCheck::new(&service);

        check.observation(&datastream(7), &object(json!({"@iot.id": 70, "FeatureOfInterest": {"@iot.id": 4711}})));
        check.observation(&datastream(7), &object(json!({"@iot.id": 71, "FeatureOfInterest": {}})));

        assert_eq!(recorded(check), CollectionWarnings::default());
    }

    #[test]
    fn a_read_that_did_not_ask_for_features_of_interest_counts_nothing() {
        let service = service("{}");
        let mut check = FeatureOfInterestCheck::new(&service);

        check.observation(&datastream(7), &object(json!({"@iot.id": 70, "result": 1.0})));
        check.newest_observation(
            StreamKind::Datastream,
            &object(json!({"@iot.id": 7, "Observations": [{"@iot.id": 71, "result": 2.0}]})),
        );

        assert_eq!(recorded(check), CollectionWarnings::default());
    }

    #[test]
    fn an_observation_without_an_id_is_named_as_one() {
        let service = service(r#"{"featuresOfInterest": true}"#);
        let mut check = FeatureOfInterestCheck::new(&service);

        check.observation(&datastream(7), &object(json!({"result": 1.0})));

        assert_eq!(
            recorded(check).unserved_features_of_interest()[0].first().observation,
            ObservationName::WithoutId
        );
    }

    #[test]
    fn the_newest_observation_of_a_latest_data_stream_is_counted_with_its_kind() {
        let service = service(r#"{"featuresOfInterest": true}"#);
        let mut check = FeatureOfInterestCheck::new(&service);

        check.newest_observation(
            StreamKind::MultiDatastream,
            &object(json!({"@iot.id": 4, "Observations": [{"@iot.id": 40, "result": [1.0, 2.0]}]})),
        );
        check.newest_observation(
            StreamKind::Datastream,
            &object(json!({"@iot.id": 5, "Observations": [{"@iot.id": 50, "FeatureOfInterest": {"@iot.id": 4711}}]})),
        );

        let warnings = recorded(check);
        let [unserved] = warnings.unserved_features_of_interest() else {
            panic!("expected one warning");
        };
        assert_eq!(unserved.first().to_string(), "Observation 40 of MultiDatastream 4");
        assert_eq!(unserved.count().get(), 1);
    }

    #[test]
    fn a_latest_data_stream_without_an_observation_or_a_readable_id_counts_nothing() {
        let service = service(r#"{"featuresOfInterest": true}"#);
        let mut check = FeatureOfInterestCheck::new(&service);

        check.newest_observation(StreamKind::Datastream, &object(json!({"@iot.id": 5, "Observations": []})));
        check.newest_observation(StreamKind::Datastream, &object(json!({"@iot.id": 6})));
        check.newest_observation(StreamKind::Datastream, &object(json!({"@iot.id": true, "Observations": [{"@iot.id": 70}]})));

        assert_eq!(recorded(check), CollectionWarnings::default());
    }
}
