use crate::sensorthings::{
    error::SensorThingsIngestError,
    ingest_losses::{SensorThingsLosses, UnreadableMultiResult},
    record_builder::{DatastreamParts, ObservationParts},
};
use cassiopeia_sensorthings::{snapshot::line_number::LineNumber, stream_id::StreamId};
use serde_json::{Map, Value};

/// The readings the served `observation`, read on `line`, gives of the data stream `datastream`,
/// each under its key, as a record holds them.
///
/// `readings` are the data stream's readings, each with its key: one for a Datastream, which takes
/// the Observation's `result`, and one per element for a `MultiDatastream`, in position order, each
/// of which takes the value at its position of the Observation's `result` array. Every other member
/// of a reading, such as its `id` and its times, is the Observation's, shared by its elements.
///
/// An Observation whose `phenomenonTime` names no instant gives no reading, and neither does one of
/// a `MultiDatastream` whose `result` is no array with one value per element, which is counted. A
/// value that is `null` gives no reading of its element, as an absent member gives no member.
///
/// # Errors
///
/// Returns [`SensorThingsIngestError::IdUnreadable`] when the Observation or its
/// `FeatureOfInterest` has an id that is neither an integer nor a text.
pub(super) fn observation_readings(
    observation: Map<String, Value>,
    line: LineNumber,
    datastream: &StreamId,
    mut readings: impl ExactSizeIterator<Item = (String, DatastreamParts)>,
    losses: &mut SensorThingsLosses,
) -> Result<Map<String, Value>, SensorThingsIngestError> {
    let mut keyed = Map::new();
    let Some(mut observation) = ObservationParts::read(observation, line, losses)? else {
        return Ok(keyed);
    };
    match datastream {
        StreamId::Datastream(_) => {
            if let Some((key, parts)) = readings.next() {
                keyed.insert(key, observation.into_reading(parts));
            }
        }
        StreamId::MultiDatastream(id) => {
            let values = match observation.take_result() {
                Some(Value::Array(values)) if values.len() == readings.len() => values,
                Some(_) | None => {
                    losses.unreadable_multi_result(UnreadableMultiResult {
                        observation: observation.name(),
                        // The loss outlives the index that keeps the id; an id is a small value.
                        multi_datastream: id.clone(),
                    });
                    return Ok(keyed);
                }
            };
            for ((key, parts), value) in readings.zip(values) {
                if !value.is_null() {
                    keyed.insert(key, observation.element_reading(value, parts));
                }
            }
        }
    }
    Ok(keyed)
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{ingest_losses::SensorThingsLosses, observation_readings::observation_readings, record_builder::DatastreamParts};
    use cassiopeia_sensorthings::{
        entity_id::EntityId,
        observation_name::ObservationName,
        snapshot::line_number::LineNumber,
        stream_id::StreamId,
        stream_kind::StreamKind,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    /// The readings of `MultiDatastream` 4, `windSpeed` and `windDirection`, with their keys.
    fn wind() -> Vec<(String, DatastreamParts)> {
        ["windSpeed", "windDirection"]
            .iter()
            .enumerate()
            .map(|(index, name)| {
                (
                    (*name).to_owned(),
                    DatastreamParts {
                        unit: None,
                        kind: StreamKind::MultiDatastream,
                        members: object(json!({"id": 4, "position": index + 1})),
                        observed_property: object(json!({"name": name})),
                        sensor: None,
                    },
                )
            })
            .collect()
    }

    /// The readings an Observation with `result` gives of `MultiDatastream` 4, and what was lost.
    fn read(result: &Value) -> (Map<String, Value>, SensorThingsLosses) {
        let mut losses = SensorThingsLosses::default();
        let observation = object(json!({"@iot.id": 501, "phenomenonTime": "2026-11-12T10:00:00Z", "result": result}));
        let readings = observation_readings(
            observation,
            LineNumber::FIRST,
            &StreamId::MultiDatastream(EntityId::Integer(4)),
            wind().into_iter(),
            &mut losses,
        )
        .unwrap();
        (readings, losses)
    }

    #[test]
    fn each_value_of_a_result_is_the_reading_of_the_element_at_its_position() {
        let (readings, losses) = read(&json!([4.2, 270]));

        assert_eq!(readings["windSpeed"]["result"], json!(4.2));
        assert_eq!(readings["windSpeed"]["multiDatastream"], json!({"id": 4, "position": 1}));
        assert_eq!(readings["windDirection"]["result"], json!(270));
        assert_eq!(readings["windDirection"]["id"], json!(501));
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_result_that_is_no_array_of_one_value_per_element_gives_no_reading_and_is_counted() {
        for result in [json!([4.2]), json!([4.2, 270, 1]), json!(4.2), Value::Null] {
            let (readings, losses) = read(&result);

            assert_eq!(readings, Map::new(), "{result}");
            assert_eq!(losses.unreadable_multi_results().total(), 1, "{result}");
            assert_eq!(
                losses.unreadable_multi_results().named()[0].observation,
                ObservationName::Id(EntityId::Integer(501))
            );
        }
    }

    #[test]
    fn a_null_value_gives_no_reading_of_its_element() {
        let (readings, losses) = read(&json!([null, 270]));

        assert_eq!(readings.keys().collect::<Vec<_>>(), ["windDirection"]);
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_datastream_observation_gives_one_reading_of_its_whole_result() {
        let mut losses = SensorThingsLosses::default();
        let parts = DatastreamParts {
            unit: None,
            kind: StreamKind::Datastream,
            members: object(json!({"id": 5})),
            observed_property: object(json!({"name": "pm10"})),
            sensor: None,
        };
        let observation = object(json!({"@iot.id": 1, "phenomenonTime": "2026-11-12T10:00:00Z", "result": [1, 2]}));

        let readings = observation_readings(
            observation,
            LineNumber::FIRST,
            &StreamId::Datastream(EntityId::Integer(5)),
            [("pm10".to_owned(), parts)].into_iter(),
            &mut losses,
        )
        .unwrap();

        assert_eq!(readings["pm10"]["result"], json!([1, 2]));
        assert_eq!(readings["pm10"]["datastream"], json!({"id": 5}));
    }
}
