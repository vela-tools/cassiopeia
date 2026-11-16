use crate::sensorthings::{
    encoded_geometry::{EncodedGeometry, encoded_geometry},
    error::{SensorThingsIngestError, StaEntity},
    ingest_losses::{FeatureOfInterestName, SensorThingsLosses, UnreadableFeatureOfInterest},
    record_builder::record_members,
    served_id::optional_served_id,
};
use cassiopeia_sensorthings::{
    feature_of_interest_members::{CARRIED_FEATURE_OF_INTEREST_MEMBERS, FEATURE_MEMBER, FEATURE_OF_INTEREST_MEMBER},
    snapshot::line_number::LineNumber,
};
use serde_json::{Map, Value};

/// The `featureOfInterest` of the reading of the served `observation`, read on `line` and taken out
/// of the Observation: its id when it has one, its members, and the geometry of its `feature`; or
/// `None` when the Observation was served without one.
///
/// A `FeatureOfInterest` is where its Observation was made (OGC 18-088 section 8.2.8). Its `feature`
/// is read by the rules a Thing's Location is read by, [`encoded_geometry`]. One encoded in another
/// way than `GeoJSON`, or without an `encodingType`, gives no `feature`, which is counted as a loss.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when the `FeatureOfInterest` has an id that is
/// neither an integer nor a text.
pub(super) fn feature_of_interest(
    observation: &mut Map<String, Value>,
    line: LineNumber,
    losses: &mut SensorThingsLosses,
) -> Result<Option<Value>, SensorThingsIngestError> {
    let Some(Value::Object(mut served)) = observation.remove(FEATURE_OF_INTEREST_MEMBER) else {
        return Ok(None);
    };
    let id = optional_served_id(&served, line, StaEntity::FeatureOfInterest)?;
    let feature = encoded_geometry(&mut served, FEATURE_MEMBER);
    let mut members = record_members(served, id.as_ref(), &CARRIED_FEATURE_OF_INTEREST_MEMBERS);
    match feature {
        EncodedGeometry::Geometry(geometry) => {
            members.insert("feature".to_owned(), geometry);
        }
        EncodedGeometry::Unreadable(encoding) => losses.unreadable_feature_of_interest(UnreadableFeatureOfInterest {
            feature: id.map_or(FeatureOfInterestName::WithoutId, FeatureOfInterestName::Id),
            encoding,
        }),
        EncodedGeometry::Absent => {}
    }
    Ok(Some(Value::Object(members)))
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        error::{SensorThingsIngestError, StaEntity},
        feature_of_interest::feature_of_interest,
        ingest_losses::{FeatureOfInterestName, GeometryEncoding, SensorThingsLosses, UnreadableFeatureOfInterest},
    };
    use cassiopeia_sensorthings::{entity_id::EntityId, snapshot::line_number::LineNumber};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    /// The `featureOfInterest` of an Observation served with `feature`, what is left of the
    /// Observation, and what reading it lost.
    fn read(feature: Value) -> (Result<Option<Value>, SensorThingsIngestError>, Map<String, Value>, SensorThingsLosses) {
        let mut observation = Map::new();
        observation.insert("@iot.id".to_owned(), json!(1));
        observation.insert("result".to_owned(), json!(2));
        observation.insert("FeatureOfInterest".to_owned(), feature);
        let mut losses = SensorThingsLosses::default();
        let read = feature_of_interest(&mut observation, LineNumber::FIRST, &mut losses);
        (read, observation, losses)
    }

    #[test]
    fn a_geojson_feature_of_interest_gives_its_members_and_geometry() {
        let (read, observation, losses) = read(json!({
            "@iot.id": 4711,
            "@iot.selfLink": "https://h.example/v1.1/FeaturesOfInterest(4711)",
            "name": "Line 12, Stationsplein",
            "description": "Position of the bus when the reading was taken",
            "encodingType": "application/geo+json",
            "feature": {"type": "Point", "coordinates": [5.4797, 51.4433]},
            "Observations@iot.navigationLink": "https://h.example/v1.1/FeaturesOfInterest(4711)/Observations",
        }));

        assert_eq!(
            read.unwrap(),
            Some(json!({
                "id": 4711,
                "name": "Line 12, Stationsplein",
                "description": "Position of the bus when the reading was taken",
                "feature": {"type": "Point", "coordinates": [5.4797, 51.4433]},
            }))
        );
        assert_eq!(Value::Object(observation), json!({"@iot.id": 1, "result": 2}));
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_feature_of_interest_whose_feature_is_a_geojson_feature_gives_its_geometry() {
        let (read, _, _) = read(json!({
            "@iot.id": "stop-12",
            "encodingType": "application/vnd.geo+json",
            "feature": {"type": "Feature", "geometry": {"type": "Point", "coordinates": [5.4797, 51.4433]}, "properties": {"line": 12}},
        }));

        assert_eq!(read.unwrap().unwrap()["feature"], json!({"type": "Point", "coordinates": [5.4797, 51.4433]}));
    }

    #[test]
    fn another_encoding_gives_no_feature_and_is_counted() {
        let (read, _, losses) = read(json!({
            "@iot.id": 4711,
            "name": "Route sheet",
            "properties": {"pages": 2, "author": null},
            "encodingType": "application/pdf",
            "feature": "https://h.example/route.pdf",
        }));

        assert_eq!(read.unwrap(), Some(json!({"id": 4711, "name": "Route sheet", "properties": {"pages": 2}})));
        assert_eq!(
            losses.unreadable_features_of_interest().named(),
            [UnreadableFeatureOfInterest {
                feature: FeatureOfInterestName::Id(EntityId::Integer(4711)),
                encoding: GeometryEncoding::Stated("application/pdf".into()),
            }]
        );
        assert_eq!(losses.unreadable_features_of_interest().total(), 1);
    }

    #[test]
    fn a_feature_of_interest_without_an_encoding_is_counted() {
        let (read, _, losses) = read(json!({"@iot.id": 4711, "feature": {"type": "Point", "coordinates": [0, 0]}}));

        assert_eq!(read.unwrap(), Some(json!({"id": 4711})));
        assert_eq!(losses.unreadable_features_of_interest().named()[0].encoding, GeometryEncoding::Unstated);
    }

    #[test]
    fn an_observation_without_a_feature_of_interest_has_none() {
        for feature in [Value::Null, json!("https://h.example/v1.1/FeaturesOfInterest(4711)")] {
            let (read, _, losses) = read(feature);
            assert_eq!(read.unwrap(), None);
            assert_eq!(losses, SensorThingsLosses::default());
        }

        let Value::Object(mut observation) =
            json!({"@iot.id": 1, "FeatureOfInterest@iot.navigationLink": "https://h.example/v1.1/Observations(1)/FeatureOfInterest"})
        else {
            panic!("expected an object");
        };
        assert_eq!(
            feature_of_interest(&mut observation, LineNumber::FIRST, &mut SensorThingsLosses::default()).unwrap(),
            None
        );
    }

    #[test]
    fn a_feature_of_interest_without_an_id_is_read_without_one() {
        let (read, _, losses) = read(json!({
            "name": "Line 12, Stationsplein",
            "encodingType": "application/geo+json",
            "feature": {"type": "Point", "coordinates": [5.4797, 51.4433]},
        }));

        assert_eq!(
            read.unwrap(),
            Some(json!({"name": "Line 12, Stationsplein", "feature": {"type": "Point", "coordinates": [5.4797, 51.4433]}}))
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn an_unreadable_feature_of_interest_without_an_id_is_counted_as_one_without_an_id() {
        let (read, _, losses) = read(json!({"encodingType": "application/pdf", "feature": "https://h.example/route.pdf"}));

        assert_eq!(read.unwrap(), Some(json!({})));
        assert_eq!(
            losses.unreadable_features_of_interest().named(),
            [UnreadableFeatureOfInterest {
                feature: FeatureOfInterestName::WithoutId,
                encoding: GeometryEncoding::Stated("application/pdf".into()),
            }]
        );
    }

    #[test]
    fn a_feature_of_interest_with_an_unreadable_id_fails() {
        for id in [json!(1.5), json!(true), json!({"id": 1})] {
            let mut feature = json!({"encodingType": "application/geo+json"});
            feature["@iot.id"] = id;
            let (read, _, _) = read(feature);
            assert!(
                matches!(
                    read,
                    Err(SensorThingsIngestError::IdUnreadable {
                        entity: StaEntity::FeatureOfInterest,
                        ..
                    })
                ),
                "{read:?}"
            );
        }
    }
}
