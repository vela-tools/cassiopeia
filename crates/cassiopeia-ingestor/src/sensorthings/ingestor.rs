use crate::{
    error::IngestorError,
    ingestor::Ingestor,
    record_batches::RecordBatches,
    sensorthings::{
        error::SensorThingsIngestError,
        ingest_losses::SensorThingsLosses,
        record_builder::{DatastreamParts, reading, server_member, thing_record},
        thing_index::{IndexedDatastream, IndexedThing, ThingIndex, ThingIndexBuilder},
    },
};
use cassiopeia_common::{channel::ChannelSender, input::Input, signal::Signal};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use cassiopeia_sensorthings::{
    read_kind::{ReadKind, SnapshotRecords},
    snapshot::{
        error::SnapshotError,
        header::SnapshotHeader,
        line::{ObservationLine, SnapshotLine},
        line_number::LineNumber,
        reader::SnapshotReader,
    },
};
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::{fs::File, io::BufReader, sync::Arc};

/// Ingestor for a SensorThings snapshot file.
///
/// The header is read when the ingestor is built, so the read mode is known before any record is,
/// and a run can refuse a snapshot its output cannot keep. The datastream lines are then indexed,
/// since every record needs its Thing, unit and `ObservedProperty`, and the observation lines are
/// streamed, never held.
pub struct SensorThingsIngestor {
    /// The snapshot's header.
    header: SnapshotHeader,
    /// The lines after the header.
    lines: SnapshotReader<BufReader<File>>,
    /// The input the snapshot was collected from, named in what the run reports about it.
    source: Input,
    /// How many records go downstream at once.
    batch_size: usize,
    /// What reading the snapshot lost or changed, shared with the report made once it is read.
    losses: Arc<Mutex<SensorThingsLosses>>,
}

impl SensorThingsIngestor {
    /// Opens the snapshot a profiled payload holds and reads its header.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is not a file, the file cannot be opened, or its
    /// header cannot be read.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize) -> Result<SensorThingsIngestor, IngestorError> {
        let (path, source) = match payload.into_payload() {
            CollectedPayload::File(file) => file.into_path_and_source(),
            CollectedPayload::Bytes(_) => return Err(SensorThingsIngestError::RequiresFile.into()),
        };
        let file = File::open(&path).map_err(|source| IngestorError::Io { source, path })?;
        let (header, lines) = SnapshotReader::open(BufReader::new(file)).map_err(SensorThingsIngestError::from)?;
        Ok(SensorThingsIngestor {
            header,
            lines,
            source,
            batch_size,
            losses: Arc::new(Mutex::new(SensorThingsLosses::default())),
        })
    }

    /// How the snapshot's read chose its readings, as its header states.
    #[must_use]
    pub fn read(&self) -> ReadKind {
        self.header.read()
    }

    /// The input the snapshot was collected from.
    #[must_use]
    pub const fn source(&self) -> &Input {
        &self.source
    }

    /// What reading the snapshot lost or changed, complete once the ingestor has run.
    #[must_use]
    pub fn losses(&self) -> Arc<Mutex<SensorThingsLosses>> {
        Arc::clone(&self.losses)
    }
}

impl Ingestor for SensorThingsIngestor {
    fn ingest(self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let SensorThingsIngestor {
            header,
            mut lines,
            batch_size,
            losses,
            ..
        } = *self;
        let mut losses = losses.lock();
        let mut batches = RecordBatches::new(&sender, batch_size);
        let server = server_member(header.service_root());

        let mut index = ThingIndexBuilder::default();
        let mut first_observation = None;
        while let Some(line) = lines.next() {
            match line.map_err(SensorThingsIngestError::from)? {
                SnapshotLine::Datastream(datastream) => index.add(datastream, lines.line(), &mut losses)?,
                SnapshotLine::Observation(observation) => {
                    first_observation = Some((observation, lines.line()));
                    break;
                }
            }
        }
        let index = index.finish(&mut losses);

        match (header.read().records(), first_observation) {
            (SnapshotRecords::PerThing, Some((_, line))) => return Err(SensorThingsIngestError::UnexpectedObservation { line }.into()),
            (SnapshotRecords::PerThing, None) => {
                for thing in index.into_things() {
                    // Each Thing is one record, which owns its copy of the server.
                    batches.push(latest_record(server.clone(), thing, &mut losses)?)?;
                }
            }
            (SnapshotRecords::PerObservation, first) => {
                let mut next = first;
                while let Some((observation, line)) = next {
                    if let Some(record) = observation_record(&server, &index, observation, line, &mut losses)? {
                        batches.push(record)?;
                    }
                    next = next_observation(&mut lines)?;
                }
            }
        }
        batches.finish()
    }
}

/// The record of one Thing of a latest snapshot, with the newest reading of each of its data streams.
fn latest_record(server: Value, thing: IndexedThing, losses: &mut SensorThingsLosses) -> Result<Record, SensorThingsIngestError> {
    let mut readings = Map::new();
    for datastream in thing.datastreams {
        let IndexedDatastream {
            key,
            unit,
            members,
            observed_property,
            latest,
        } = datastream;
        let Some((observation, line)) = latest else {
            continue;
        };
        let parts = DatastreamParts {
            unit,
            members,
            observed_property,
        };
        if let Some(reading) = reading(observation, line, parts, losses)? {
            readings.insert(key.to_string(), reading);
        }
    }
    Ok(thing_record(server, thing.members, thing.location, readings))
}

/// The record of one Observation of a range or incremental snapshot, or `None` when its
/// `phenomenonTime` names no instant, since a record of one Observation without its reading
/// describes nothing.
fn observation_record(
    server: &Value,
    index: &ThingIndex,
    observation: ObservationLine,
    line: LineNumber,
    losses: &mut SensorThingsLosses,
) -> Result<Option<Record>, SensorThingsIngestError> {
    let (observation, datastream_id) = observation.into_parts();
    let Some((thing, datastream)) = index.datastream(&datastream_id) else {
        return Err(SensorThingsIngestError::UnknownDatastream {
            line,
            datastream: datastream_id,
        });
    };
    // Every record owns its data, so each Observation's record takes its own copy of what it shares
    // with the other readings of its data stream and Thing.
    let parts = DatastreamParts {
        unit: datastream.unit.clone(),
        members: datastream.members.clone(),
        observed_property: datastream.observed_property.clone(),
    };
    let Some(reading) = reading(observation, line, parts, losses)? else {
        return Ok(None);
    };
    let mut readings = Map::new();
    readings.insert(datastream.key.to_string(), reading);
    Ok(Some(thing_record(server.clone(), thing.members.clone(), thing.location.clone(), readings)))
}

/// The next observation line and its number, or `None` past the last line.
fn next_observation(lines: &mut SnapshotReader<BufReader<File>>) -> Result<Option<(ObservationLine, LineNumber)>, SensorThingsIngestError> {
    match lines.next().transpose()? {
        None => Ok(None),
        Some(SnapshotLine::Observation(observation)) => Ok(Some((observation, lines.line()))),
        // The reader refuses a datastream line after an observation line before it yields one.
        Some(SnapshotLine::Datastream(_)) => Err(SnapshotError::OrderViolation { line: lines.line() }.into()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::IngestorError,
        ingestor::Ingestor,
        sensorthings::{
            error::SensorThingsIngestError,
            ingest_losses::{LocationEncoding, SensorThingsLosses, UnreadableLocation},
            ingestor::SensorThingsIngestor,
            reading_keys::RenamedReading,
        },
    };
    use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use cassiopeia_ir::{
        payload::{BytesPayload, CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use cassiopeia_sensorthings::{entity_id::EntityId, read_kind::ReadKind};
    use mediatype::media_type;
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};
    use std::{fs, mem, sync::mpsc::sync_channel, thread};
    use temp_dir::TempDir;

    /// A header of a snapshot read from RIVM Samen Meten by a `read` read.
    fn header(read: &str) -> String {
        format!(
            r#"{{"sensorthingsSnapshot":1,"serviceRoot":"https://api-samenmeten.rivm.nl/v1.0","version":"v1.0","read":"{read}","collectedAt":"2026-11-12T10:00:00Z"}}"#
        )
    }

    /// The Thing RIVM serves as `ILM_I68`, with its one Location.
    fn rivm_thing() -> Value {
        json!({
            "@iot.id": 12512,
            "name": "ILM_I68",
            "description": "ILM_I68",
            "Locations": [{"@iot.id": 36182, "encodingType": "application/vnd.geo+json", "location": {"type": "Point", "coordinates": [5.608, 51.313]}}],
        })
    }

    /// A datastream line of data stream `id`, observing `property`, of `thing`, expanded with
    /// `observations`.
    fn datastream(id: Value, property: &str, thing: Value, observations: Value) -> Value {
        let mut datastream = json!({
            "name": format!("ILM_I68-13-{property}"),
            "description": null,
            "unitOfMeasurement": {"symbol": "ug/m3", "definition": "https://qudt.org/vocab/unit/MicroGM-PER-M3"},
            "observationType": "http://www.opengis.net/def/observationType/OGC-OM/2.0/OM_Measurement",
            "ObservedProperty": {"@iot.id": 1, "name": property, "definition": format!("https://www.eea.europa.eu/help/glossary/eea-glossary/{property}")},
        });
        datastream["@iot.id"] = id;
        datastream["Thing"] = thing;
        datastream["Observations"] = observations;
        let mut line = Map::new();
        line.insert("datastream".to_owned(), datastream);
        Value::Object(line)
    }

    /// An Observation at `time` with `result`.
    fn observation(id: i64, time: &str, result: f64) -> Value {
        json!({"@iot.id": id, "phenomenonTime": time, "result": result})
    }

    /// The snapshot of a `read` read holding `lines` after its header.
    fn snapshot(read: &str, lines: &[Value]) -> String {
        let mut text = header(read);
        text.push('\n');
        for line in lines {
            text.push_str(&line.to_string());
            text.push('\n');
        }
        text
    }

    /// The ingestor of a snapshot file holding `text`, with the directory the file lives in.
    fn ingestor(text: &str, batch_size: usize) -> Result<(TempDir, SensorThingsIngestor), IngestorError> {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("rivm.jsonl");
        fs::write(&path, text).unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::local(path, Some(DataFormat::SensorThings), EncodingHint::NONE)),
            Profile::new(DataFormat::SensorThings, media_type!(APPLICATION / JSON), 1.0),
        );
        Ok((directory, SensorThingsIngestor::from_payload(payload, batch_size)?))
    }

    /// The record batches a snapshot holding `text` is read into, in batches of `batch_size`, and
    /// what reading it lost; or the failure the read ended with.
    fn read_batches(text: &str, batch_size: usize) -> Result<(Vec<Vec<Record>>, SensorThingsLosses), IngestorError> {
        let (_directory, ingestor) = ingestor(text, batch_size)?;
        let losses = ingestor.losses();
        let (tx, rx) = sync_channel(64);
        let reading = thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(tx)));
        let batches = rx
            .iter()
            .map(|signal| match signal {
                Signal::Data(records) => records,
                Signal::Start | Signal::Stop | Signal::Error(_) | Signal::Meta(_) => panic!("expected a data signal"),
            })
            .collect();
        reading.join().unwrap()?;
        let losses = mem::take(&mut *losses.lock());
        Ok((batches, losses))
    }

    /// The records a snapshot holding `text` is read into, and what reading it lost.
    fn read(text: &str) -> (Vec<Value>, SensorThingsLosses) {
        let (batches, losses) = read_batches(text, 100).unwrap();
        let records = batches.into_iter().flatten().map(|record| Value::Object(record.into_data())).collect();
        (records, losses)
    }

    #[test]
    fn a_latest_snapshot_yields_one_record_per_thing() {
        let text = snapshot(
            "latest",
            &[
                datastream(
                    json!(56776),
                    "pm10",
                    rivm_thing(),
                    json!([observation(98765, "2026-11-12T09:00:00Z/2026-11-12T10:00:00Z", 21.4)]),
                ),
                datastream(
                    json!(56775),
                    "pm25_kal",
                    rivm_thing(),
                    json!([observation(98764, "2026-11-12T09:00:00Z/2026-11-12T10:00:00Z", 9.1)]),
                ),
            ],
        );

        let (records, losses) = read(&text);

        let reading = |id: i64, result: f64, datastream: i64, property: &str| {
            json!({
                "id": id,
                "result": result,
                "time": "2026-11-12T10:00:00Z",
                "phenomenonTime": "2026-11-12T09:00:00Z/2026-11-12T10:00:00Z",
                "unit": {"symbol": "ug/m3", "definition": "https://qudt.org/vocab/unit/MicroGM-PER-M3"},
                "datastream": {"id": datastream, "name": format!("ILM_I68-13-{property}"), "observationType": "http://www.opengis.net/def/observationType/OGC-OM/2.0/OM_Measurement"},
                "observedProperty": {"id": 1, "name": property, "definition": format!("https://www.eea.europa.eu/help/glossary/eea-glossary/{property}")},
            })
        };
        assert_eq!(
            records,
            [json!({
                "server": {"url": "https://api-samenmeten.rivm.nl/v1.0", "host": "api-samenmeten.rivm.nl", "version": "v1.0"},
                "thing": {"id": 12512, "name": "ILM_I68", "description": "ILM_I68"},
                "location": {"type": "Point", "coordinates": [5.608, 51.313]},
                "readings": {"pm10": reading(98765, 21.4, 56776, "pm10"), "pm25_kal": reading(98764, 9.1, 56775, "pm25_kal")},
            })]
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_data_stream_without_observation_has_no_reading() {
        let text = snapshot(
            "latest",
            &[
                datastream(json!(56776), "pm10", rivm_thing(), json!([observation(1, "2026-11-12T10:00:00Z", 21.4)])),
                datastream(json!(56775), "pm25_kal", rivm_thing(), json!([observation(2, "2026-11-12T10:00:00Z", 9.1)])),
                datastream(json!(56777), "pm10_kal", rivm_thing(), json!([])),
            ],
        );

        let (records, _) = read(&text);

        assert_eq!(records[0]["readings"].as_object().unwrap().keys().collect::<Vec<_>>(), ["pm10", "pm25_kal"]);
    }

    #[test]
    fn a_thing_without_observations_is_a_record_without_readings() {
        let (records, _) = read(&snapshot("latest", &[datastream(json!(56776), "pm10", rivm_thing(), json!([]))]));

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].as_object().unwrap().keys().collect::<Vec<_>>(), ["server", "thing", "location"]);
    }

    #[test]
    fn repeated_property_names_are_suffixed_by_data_stream_id() {
        let text = snapshot(
            "latest",
            &[
                datastream(json!(9), "pm10", rivm_thing(), json!([observation(1, "2026-11-12T10:00:00Z", 1.0)])),
                datastream(json!(5), "pm10", rivm_thing(), json!([observation(2, "2026-11-12T10:00:00Z", 2.0)])),
            ],
        );

        let (records, losses) = read(&text);

        assert_eq!(records[0]["readings"]["pm10"]["datastream"]["id"], 5);
        assert_eq!(records[0]["readings"]["pm10@9"]["datastream"]["id"], 9);
        assert_eq!(losses.renamed_readings().total(), 1);
        assert_eq!(
            losses.renamed_readings().named()[0].to_string(),
            "Thing 12512 reads data stream 9 as `readings['pm10@9']`"
        );
        assert!(matches!(
            &losses.renamed_readings().named()[0],
            RenamedReading {
                thing: EntityId::Integer(12512),
                datastream: EntityId::Integer(9),
                ..
            }
        ));
    }

    #[test]
    fn the_lowest_location_id_is_used() {
        let thing = json!({
            "@iot.id": 12512,
            "Locations": [
                {"@iot.id": 3, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": [3, 3]}},
                {"@iot.id": 2, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": [2, 2]}},
            ],
        });

        let (records, _) = read(&snapshot("latest", &[datastream(json!(5), "pm10", thing, json!([]))]));

        assert_eq!(records[0]["location"], json!({"type": "Point", "coordinates": [2, 2]}));
    }

    #[test]
    fn a_feature_location_yields_its_geometry() {
        let thing = json!({
            "@iot.id": "t-1",
            "Locations": [{"@iot.id": "l-1", "encodingType": "application/geo+json", "location": {"type": "Feature", "geometry": {"type": "Point", "coordinates": [1, 2]}, "properties": {}}}],
        });

        let (records, _) = read(&snapshot("latest", &[datastream(json!(5), "pm10", thing, json!([]))]));

        assert_eq!(records[0]["location"], json!({"type": "Point", "coordinates": [1, 2]}));
    }

    #[test]
    fn the_older_geojson_media_type_is_read() {
        let (records, _) = read(&snapshot("latest", &[datastream(json!(5), "pm10", rivm_thing(), json!([]))]));

        assert!(records[0].get("location").is_some());
    }

    #[test]
    fn another_location_encoding_is_left_out_with_a_warning() {
        let thing = json!({"@iot.id": 12512, "Locations": [{"@iot.id": 1, "encodingType": "text/plain", "location": "Valkenswaard"}]});
        let text = snapshot(
            "latest",
            &[
                datastream(json!(5), "pm10", thing.clone(), json!([])),
                datastream(json!(6), "no2", thing, json!([])),
            ],
        );

        let (records, losses) = read(&text);

        assert!(records[0].get("location").is_none());
        assert_eq!(losses.unreadable_locations().total(), 1);
        assert_eq!(
            losses.unreadable_locations().named(),
            [UnreadableLocation {
                thing: EntityId::Integer(12512),
                encoding: LocationEncoding::Stated("text/plain".into()),
            }]
        );
    }

    #[test]
    fn null_members_are_absent() {
        let observations = json!([{"@iot.id": 1, "phenomenonTime": "2026-11-12T10:00:00Z", "resultTime": null, "result": null}]);

        let (records, _) = read(&snapshot("latest", &[datastream(json!(5), "pm10", rivm_thing(), observations)]));

        let reading = records[0]["readings"]["pm10"].as_object().unwrap();
        assert!(!reading.contains_key("resultTime"));
        assert!(!reading.contains_key("result"));
        assert!(!reading["datastream"].as_object().unwrap().contains_key("description"));
    }

    #[test]
    fn an_unreadable_phenomenon_time_drops_its_reading() {
        let text = snapshot(
            "latest",
            &[
                datastream(json!(5), "pm10", rivm_thing(), json!([observation(1, "2026-11-12", 1.0)])),
                datastream(json!(6), "no2", rivm_thing(), json!([observation(2, "2026-11-12T10:00:00Z", 2.0)])),
            ],
        );

        let (records, losses) = read(&text);

        assert_eq!(records[0]["readings"].as_object().unwrap().keys().collect::<Vec<_>>(), ["no2"]);
        assert_eq!(losses.unreadable_phenomenon_times().named(), [Box::<str>::from("2026-11-12")]);
        assert_eq!(losses.unreadable_phenomenon_times().total(), 1);
    }

    #[test]
    fn string_and_integer_ids_keep_their_json_type() {
        let thing = json!({"@iot.id": "t-1", "name": "FROST thing"});

        let (records, _) = read(&snapshot(
            "latest",
            &[datastream(json!(5), "pm10", thing, json!([observation(1, "2026-11-12T10:00:00Z", 1.0)]))],
        ));

        assert_eq!(records[0]["thing"]["id"], "t-1");
        assert_eq!(records[0]["readings"]["pm10"]["datastream"]["id"], 5);
    }

    #[test]
    fn a_missing_observed_property_fails_the_source() {
        let mut line = datastream(json!(5), "pm10", rivm_thing(), json!([]));
        line["datastream"].as_object_mut().unwrap().remove("ObservedProperty");

        let error = read_batches(&snapshot("latest", &[line]), 100).unwrap_err();

        assert!(matches!(
            error,
            IngestorError::SensorThings(SensorThingsIngestError::ObservedPropertyMissing {
                datastream: EntityId::Integer(5)
            })
        ));
        assert!(error.to_string().contains("data stream 5"), "{error}");
    }

    #[test]
    fn records_are_sent_in_batches() {
        let lines: Vec<Value> = (1..=5)
            .map(|thing| datastream(json!(thing), "pm10", json!({"@iot.id": thing}), json!([])))
            .collect();

        let (batches, _) = read_batches(&snapshot("latest", &lines), 2).unwrap();

        assert_eq!(batches.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2, 1]);
    }

    #[test]
    fn a_range_snapshot_yields_one_record_per_observation() {
        let text = snapshot(
            "range",
            &[
                datastream(json!(56776), "pm10", rivm_thing(), Value::Null),
                datastream(json!(56775), "pm25_kal", rivm_thing(), Value::Null),
                json!({"observation": observation(1, "2026-11-12T08:00:00Z", 1.0), "datastream": 56776}),
                json!({"observation": observation(2, "2026-11-12T09:00:00Z", 2.0), "datastream": 56776}),
                json!({"observation": observation(3, "2026-11-12T08:00:00Z", 3.0), "datastream": 56775}),
            ],
        );

        let (records, _) = read(&text);

        let readings: Vec<(String, Value)> = records
            .iter()
            .map(|record| {
                let readings = record["readings"].as_object().unwrap();
                assert_eq!(readings.len(), 1);
                let (key, reading) = readings.iter().next().unwrap();
                (key.clone(), reading["result"].clone())
            })
            .collect();
        assert_eq!(
            readings,
            [
                ("pm10".to_owned(), json!(1.0)),
                ("pm10".to_owned(), json!(2.0)),
                ("pm25_kal".to_owned(), json!(3.0))
            ]
        );
        assert!(records.iter().all(|record| record["thing"]["id"] == 12512 && record.get("location").is_some()));
    }

    #[test]
    fn a_range_observation_with_an_unreadable_time_gives_no_record() {
        let text = snapshot(
            "range",
            &[
                datastream(json!(5), "pm10", rivm_thing(), Value::Null),
                json!({"observation": observation(1, "soon", 1.0), "datastream": 5}),
            ],
        );

        let (records, losses) = read(&text);

        assert_eq!(records, Vec::<Value>::new());
        assert_eq!(losses.unreadable_phenomenon_times().total(), 1);
    }

    #[test]
    fn an_observation_of_an_unknown_data_stream_fails_naming_its_line() {
        let text = snapshot(
            "range",
            &[
                datastream(json!(5), "pm10", rivm_thing(), Value::Null),
                json!({"observation": observation(1, "2026-11-12T10:00:00Z", 1.0), "datastream": 7}),
            ],
        );

        let error = read_batches(&text, 100).unwrap_err();

        assert!(matches!(
            error,
            IngestorError::SensorThings(SensorThingsIngestError::UnknownDatastream { line, datastream: EntityId::Integer(7) }) if line.get() == 3
        ));
    }

    #[test]
    fn an_observation_line_in_a_latest_snapshot_fails() {
        let text = snapshot(
            "latest",
            &[
                datastream(json!(5), "pm10", rivm_thing(), json!([])),
                json!({"observation": observation(1, "2026-11-12T10:00:00Z", 1.0), "datastream": 5}),
            ],
        );

        assert!(matches!(
            read_batches(&text, 100),
            Err(IngestorError::SensorThings(SensorThingsIngestError::UnexpectedObservation { .. }))
        ));
    }

    #[test]
    fn the_header_is_read_when_the_ingestor_is_built() {
        let (_directory, ingestor) = ingestor(&snapshot("incremental", &[]), 100).unwrap();

        assert_eq!(ingestor.read(), ReadKind::Incremental);
    }

    #[test]
    fn a_file_without_a_header_fails_when_the_ingestor_is_built() {
        assert!(matches!(
            ingestor("{\"value\":[]}\n", 100),
            Err(IngestorError::SensorThings(SensorThingsIngestError::Snapshot(_)))
        ));
    }

    #[test]
    fn a_bytes_payload_is_refused() {
        let payload = ProfiledPayload::new(
            CollectedPayload::Bytes(BytesPayload::new(header("latest").into_bytes(), None, EncodingHint::NONE)),
            Profile::new(DataFormat::SensorThings, media_type!(APPLICATION / JSON), 1.0),
        );

        assert!(matches!(
            SensorThingsIngestor::from_payload(payload, 100),
            Err(IngestorError::SensorThings(SensorThingsIngestError::RequiresFile))
        ));
    }
}
