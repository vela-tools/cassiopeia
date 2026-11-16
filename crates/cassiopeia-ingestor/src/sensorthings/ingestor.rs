use crate::{
    error::IngestorError,
    ingestor::Ingestor,
    record_batches::RecordBatches,
    sensorthings::{
        error::SensorThingsIngestError,
        historical_location::historical_location,
        ingest_losses::SensorThingsLosses,
        observation_readings::observation_readings,
        record_builder::{RecordContent, server_member, thing_record},
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
        line::{HistoricalLocationLine, LineKind, ObservationLine, SnapshotLine},
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
/// and a run can refuse a snapshot its output cannot keep. The data stream lines are then indexed,
/// since every record needs its Thing, units, `ObservedProperty` and `Sensor`, and the observation
/// lines and the historicalLocation lines after them are streamed, never held.
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
        let mut first_record_line = None;
        while let Some(line) = lines.next() {
            let record_line = match line.map_err(SensorThingsIngestError::from)? {
                SnapshotLine::Datastream(datastream) => {
                    index.add(datastream, lines.line(), &mut losses)?;
                    continue;
                }
                SnapshotLine::Observation(observation) => RecordLine::Observation(observation),
                SnapshotLine::HistoricalLocation(historical_location) => RecordLine::HistoricalLocation(historical_location),
            };
            first_record_line = Some((record_line, lines.line()));
            break;
        }
        let index = index.finish(&mut losses);

        match (header.read().records(), first_record_line) {
            (SnapshotRecords::PerThing, Some((RecordLine::Observation(_), line))) => return Err(SensorThingsIngestError::UnexpectedObservation { line }.into()),
            (SnapshotRecords::PerThing, Some((RecordLine::HistoricalLocation(_), line))) => {
                return Err(SensorThingsIngestError::UnexpectedHistoricalLocation { line }.into());
            }
            (SnapshotRecords::PerThing, None) => {
                for thing in index.into_things() {
                    // Each Thing is one record, which owns its copy of the server.
                    batches.push(latest_record(server.clone(), thing, &mut losses)?)?;
                }
            }
            (SnapshotRecords::PerObservation, first) => {
                let mut next = first;
                while let Some((record_line, line)) = next {
                    let kind = record_line.kind();
                    let record = match record_line {
                        RecordLine::Observation(observation) => observation_record(&server, &index, observation, line, &mut losses)?,
                        RecordLine::HistoricalLocation(historical_location) => {
                            historical_location_record(&server, &index, historical_location, line, &mut losses)?
                        }
                    };
                    if let Some(record) = record {
                        batches.push(record)?;
                    }
                    next = next_record_line(&mut lines, kind)?;
                }
            }
        }
        batches.finish()
    }
}

/// A line of a range or incremental snapshot that gives a record: an observation line, or a
/// historicalLocation line after them.
enum RecordLine {
    /// An observation line.
    Observation(ObservationLine),
    /// A historicalLocation line.
    HistoricalLocation(HistoricalLocationLine),
}

impl RecordLine {
    /// The kind of the line.
    const fn kind(&self) -> LineKind {
        match self {
            RecordLine::Observation(_) => LineKind::Observation,
            RecordLine::HistoricalLocation(_) => LineKind::HistoricalLocation,
        }
    }
}

/// The record of one Thing of a latest snapshot, with the newest readings of each of its data
/// streams: one of a Datastream, and one per element of a `MultiDatastream`.
fn latest_record(server: Value, thing: IndexedThing, losses: &mut SensorThingsLosses) -> Result<Record, SensorThingsIngestError> {
    let mut readings = Map::new();
    for datastream in thing.datastreams {
        let IndexedDatastream { id, readings: keyed, latest } = datastream;
        let Some((observation, line)) = latest else {
            continue;
        };
        let keyed = keyed.into_iter().map(|reading| (reading.key.to_string(), reading.parts));
        readings.extend(observation_readings(observation, line, &id, keyed, losses)?);
    }
    Ok(thing_record(server, thing.members, thing.locations, RecordContent::Readings(readings)))
}

/// The record of one Observation of a range or incremental snapshot, holding its reading of a
/// Datastream or its readings of the elements of a `MultiDatastream`, or `None` when it gives no
/// reading, since a record of one Observation without its readings describes nothing.
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
    let keyed = datastream.readings.iter().map(|reading| (reading.key.to_string(), reading.parts.clone()));
    let readings = observation_readings(observation, line, &datastream.id, keyed, losses)?;
    if readings.is_empty() {
        return Ok(None);
    }
    Ok(Some(thing_record(
        server.clone(),
        thing.members.clone(),
        thing.locations.clone(),
        RecordContent::Readings(readings),
    )))
}

/// The record of one `HistoricalLocation` of a range or incremental snapshot, holding its Thing's
/// parts, as a record of its Observations does, and the `historicalLocation`, or `None` when it
/// gives none, which reading it counts as a loss.
fn historical_location_record(
    server: &Value,
    index: &ThingIndex,
    line: HistoricalLocationLine,
    number: LineNumber,
    losses: &mut SensorThingsLosses,
) -> Result<Option<Record>, SensorThingsIngestError> {
    let (as_served, thing_id) = line.into_parts();
    let Some(thing) = index.thing(&thing_id) else {
        return Err(SensorThingsIngestError::UnknownThing { line: number, thing: thing_id });
    };
    let Some(historical_location) = historical_location(as_served, &thing_id, number, losses)? else {
        return Ok(None);
    };
    // Every record owns its data, so each HistoricalLocation's record takes its own copy of what it
    // shares with the other records of its Thing.
    Ok(Some(thing_record(
        server.clone(),
        thing.members.clone(),
        thing.locations.clone(),
        RecordContent::HistoricalLocation(historical_location),
    )))
}

/// The next line that gives a record and its number, or `None` past the last line, after a line of
/// kind `after`.
fn next_record_line(lines: &mut SnapshotReader<BufReader<File>>, after: LineKind) -> Result<Option<(RecordLine, LineNumber)>, SensorThingsIngestError> {
    match lines.next().transpose()? {
        None => Ok(None),
        Some(SnapshotLine::Observation(observation)) => Ok(Some((RecordLine::Observation(observation), lines.line()))),
        Some(SnapshotLine::HistoricalLocation(historical_location)) => Ok(Some((RecordLine::HistoricalLocation(historical_location), lines.line()))),
        // The reader refuses a data stream line after a line of a later kind before it yields one.
        Some(SnapshotLine::Datastream(_)) => Err(SnapshotError::OrderViolation {
            line: lines.line(),
            kind: LineKind::Datastream,
            after,
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::IngestorError,
        ingestor::Ingestor,
        sensorthings::{
            error::{SensorThingsIngestError, StaEntity},
            ingest_losses::{
                FeatureOfInterestName,
                GeometryEncoding,
                HistoricalLocationFault,
                HistoricalLocationName,
                SensorThingsLosses,
                UnreadableFeatureOfInterest,
                UnreadableHistoricalLocation,
                UnreadableLocation,
            },
            ingestor::SensorThingsIngestor,
            reading_keys::{ReadingSource, RenamedReading},
        },
    };
    use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use cassiopeia_ir::{
        payload::{BytesPayload, CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use cassiopeia_sensorthings::{
        entity_id::EntityId,
        read_kind::ReadKind,
        snapshot::{error::SnapshotError, line::LineKind},
        stream_id::StreamId,
    };
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
                "locations": [{"id": 36182, "encodingType": "application/vnd.geo+json", "location": {"type": "Point", "coordinates": [5.608, 51.313]}}],
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
        assert_eq!(
            records[0].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["server", "thing", "location", "locations"]
        );
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
            "Thing 12512 reads Datastream 9 as `readings['pm10@9']`"
        );
        assert!(matches!(
            &losses.renamed_readings().named()[0],
            RenamedReading {
                thing: EntityId::Integer(12512),
                source: ReadingSource::Datastream(EntityId::Integer(9)),
                ..
            }
        ));
    }

    /// A Location with `id`, encoded as `encoding`, holding `location`.
    fn location(id: Value, encoding: &str, location: Value) -> Value {
        let mut served = json!({"encodingType": encoding});
        served["@iot.id"] = id;
        served["location"] = location;
        served
    }

    /// The records a latest snapshot of one data stream of Thing 12512, served with `locations`, is
    /// read into, and what reading it lost.
    fn read_locations(locations: Value) -> (Vec<Value>, SensorThingsLosses) {
        let mut thing = json!({"@iot.id": 12512, "name": "ILM_I68"});
        thing["Locations"] = locations;
        read(&snapshot("latest", &[datastream(json!(5), "pm10", thing, json!([]))]))
    }

    #[test]
    fn a_geojson_location_gives_the_location_though_a_text_location_has_a_lower_id() {
        let (records, losses) = read_locations(json!([
            location(json!(2), "application/geo+json", point([5.4797, 51.4433])),
            location(json!(1), "text/plain", json!("Stationsplein 1, 3rd floor, room 3.12")),
        ]));

        assert_eq!(records[0]["location"], point([5.4797, 51.4433]));
        assert_eq!(
            records[0]["locations"],
            json!([
                {"id": 1, "encodingType": "text/plain", "location": "Stationsplein 1, 3rd floor, room 3.12"},
                {"id": 2, "encodingType": "application/geo+json", "location": point([5.4797, 51.4433])},
            ])
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn one_geojson_location_gives_the_location_and_its_members() {
        let (records, losses) = read_locations(json!([{
            "@iot.id": 36182,
            "@iot.selfLink": "https://h.example/v1.1/Locations(36182)",
            "name": "Stationsplein 1, Eindhoven",
            "description": "Roof of the station building",
            "encodingType": "application/geo+json",
            "properties": {"floor": "roof"},
            "location": point([5.4797, 51.4433]),
            "Things@iot.navigationLink": "https://h.example/v1.1/Locations(36182)/Things",
            "HistoricalLocations@iot.navigationLink": "https://h.example/v1.1/Locations(36182)/HistoricalLocations",
        }]));

        assert_eq!(records[0]["location"], point([5.4797, 51.4433]));
        assert_eq!(
            records[0]["locations"],
            json!([{
                "id": 36182,
                "name": "Stationsplein 1, Eindhoven",
                "description": "Roof of the station building",
                "encodingType": "application/geo+json",
                "properties": {"floor": "roof"},
                "location": point([5.4797, 51.4433]),
            }])
        );
        assert_eq!(
            records[0].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["server", "thing", "location", "locations"]
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_thing_whose_only_location_is_text_has_no_location_and_one_warning() {
        let thing = json!({"@iot.id": 12512, "Locations": [location(json!(36183), "text/plain", json!("Valkenswaard"))]});
        let text = snapshot(
            "latest",
            &[
                datastream(json!(5), "pm10", thing.clone(), json!([])),
                datastream(json!(6), "no2", thing, json!([])),
            ],
        );

        let (records, losses) = read(&text);

        assert!(records[0].get("location").is_none());
        assert_eq!(
            records[0]["locations"],
            json!([{"id": 36183, "encodingType": "text/plain", "location": "Valkenswaard"}])
        );
        assert_eq!(losses.unreadable_locations().total(), 1);
        assert_eq!(
            losses.unreadable_locations().named(),
            [UnreadableLocation {
                thing: EntityId::Integer(12512),
                location: EntityId::Integer(36183),
                encoding: GeometryEncoding::Stated("text/plain".into()),
            }]
        );
    }

    #[test]
    fn a_feature_location_yields_its_geometry_in_both_members() {
        let (records, _) = read_locations(json!([location(
            json!("l-1"),
            "application/geo+json",
            json!({"type": "Feature", "geometry": point([1.0, 2.0]), "properties": {}})
        )]));

        assert_eq!(records[0]["location"], point([1.0, 2.0]));
        assert_eq!(records[0]["locations"][0]["location"], point([1.0, 2.0]));
    }

    #[test]
    fn locations_are_in_id_order_and_the_lowest_geojson_one_gives_the_location() {
        let (records, _) = read_locations(json!([
            location(json!("b"), "application/geo+json", point([2.0, 2.0])),
            location(json!(7), "application/geo+json", point([7.0, 7.0])),
            location(json!("a"), "application/geo+json", point([1.0, 1.0])),
        ]));

        let ids: Vec<&Value> = records[0]["locations"].as_array().unwrap().iter().map(|location| &location["id"]).collect();
        assert_eq!(ids, [&json!(7), &json!("a"), &json!("b")]);
        assert_eq!(records[0]["location"], point([7.0, 7.0]));
    }

    #[test]
    fn a_null_location_member_is_left_out() {
        let (records, _) = read_locations(json!([{
            "@iot.id": 36182,
            "name": "Stationsplein 1, Eindhoven",
            "description": null,
            "encodingType": "application/geo+json",
            "location": point([5.4797, 51.4433]),
        }]));

        assert_eq!(
            records[0]["locations"],
            json!([{"id": 36182, "name": "Stationsplein 1, Eindhoven", "encodingType": "application/geo+json", "location": point([5.4797, 51.4433])}])
        );
    }

    #[test]
    fn the_older_geojson_media_type_is_read() {
        let (records, _) = read(&snapshot("latest", &[datastream(json!(5), "pm10", rivm_thing(), json!([]))]));

        assert!(records[0].get("location").is_some());
    }

    #[test]
    fn every_read_gives_a_thing_the_same_location_and_locations() {
        let thing = json!({
            "@iot.id": 12512,
            "Locations": [
                location(json!(2), "application/geo+json", point([5.4797, 51.4433])),
                location(json!(1), "text/plain", json!("Stationsplein 1")),
            ],
        });
        let (latest, _) = read(&snapshot(
            "latest",
            &[datastream(
                json!(5),
                "pm10",
                thing.clone(),
                json!([observation(2, "2026-11-12T10:00:00Z", 2.0)]),
            )],
        ));
        assert_eq!(latest[0]["location"], point([5.4797, 51.4433]));

        for read_kind in ["range", "incremental"] {
            let (records, _) = read(&snapshot(
                read_kind,
                &[
                    datastream(json!(5), "pm10", thing.clone(), Value::Null),
                    json!({"observation": observation(1, "2026-11-12T09:00:00Z", 1.0), "datastream": 5}),
                    json!({"observation": observation(2, "2026-11-12T10:00:00Z", 2.0), "datastream": 5}),
                ],
            ));

            assert_eq!(records.len(), 2, "{read_kind}");
            for record in &records {
                assert_eq!(record["location"], latest[0]["location"], "{read_kind}");
                assert_eq!(record["locations"], latest[0]["locations"], "{read_kind}");
            }
        }
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
                datastream: StreamId::Datastream(EntityId::Integer(5))
            })
        ));
        assert!(error.to_string().contains("Datastream 5"), "{error}");
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

    /// A `FeatureOfInterest` with id `id`, encoded as `encoding`, holding `feature`.
    fn feature_of_interest(id: i64, encoding: &str, feature: Value) -> Value {
        let mut served = json!({"@iot.id": id, "name": format!("stop {id}"), "encodingType": encoding});
        served["feature"] = feature;
        served
    }

    /// A `GeoJSON` point at `coordinates`.
    fn point(coordinates: [f64; 2]) -> Value {
        json!({"type": "Point", "coordinates": coordinates})
    }

    /// An Observation at `time` with `result`, served with the `FeatureOfInterest` `feature`.
    fn observation_at(id: i64, time: &str, result: f64, feature: Value) -> Value {
        let mut observation = observation(id, time, result);
        observation["FeatureOfInterest"] = feature;
        observation
    }

    #[test]
    fn a_range_snapshot_of_a_moving_thing_gives_each_reading_its_own_feature() {
        let stops = [[5.4797, 51.4433], [5.4812, 51.4402], [5.4870, 51.4381]];
        let mut lines = vec![datastream(json!(56776), "pm10", rivm_thing(), Value::Null)];
        lines.extend(stops.iter().zip(1..).map(|(stop, id)| {
            let feature = feature_of_interest(4710 + id, "application/geo+json", point(*stop));
            json!({"observation": observation_at(id, &format!("2026-11-12T0{id}:00:00Z"), 1.0, feature), "datastream": 56776})
        }));

        let (records, losses) = read(&snapshot("range", &lines));

        let features: Vec<&Value> = records
            .iter()
            .map(|record| &record["readings"]["pm10"]["featureOfInterest"]["feature"])
            .collect();
        assert_eq!(features, stops.map(point).iter().collect::<Vec<_>>());
        assert_eq!(
            records[0]["readings"]["pm10"]["featureOfInterest"],
            json!({"id": 4711, "name": "stop 4711", "feature": point(stops[0])})
        );
        assert!(
            records
                .iter()
                .all(|record| record["location"] == json!({"type": "Point", "coordinates": [5.608, 51.313]})),
            "the record's location stays the Thing's"
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_latest_snapshot_gives_each_newest_reading_its_feature() {
        let feature = feature_of_interest(4711, "application/geo+json", point([5.4797, 51.4433]));
        let text = snapshot(
            "latest",
            &[
                datastream(
                    json!(56776),
                    "pm10",
                    rivm_thing(),
                    json!([observation_at(1, "2026-11-12T10:00:00Z", 21.4, feature)]),
                ),
                datastream(json!(56775), "pm25_kal", rivm_thing(), json!([observation(2, "2026-11-12T10:00:00Z", 9.1)])),
            ],
        );

        let (records, _) = read(&text);

        assert_eq!(records[0]["readings"]["pm10"]["featureOfInterest"]["feature"], point([5.4797, 51.4433]));
        assert!(
            !records[0]["readings"]["pm25_kal"].as_object().unwrap().contains_key("featureOfInterest"),
            "a newest Observation served without its feature has none"
        );
    }

    #[test]
    fn a_feature_of_interest_whose_feature_is_a_geojson_feature_gives_its_geometry() {
        let feature = feature_of_interest(
            4711,
            "application/geo+json",
            json!({"type": "Feature", "geometry": point([5.4797, 51.4433]), "properties": {"stop": "Stationsplein"}}),
        );
        let text = snapshot(
            "range",
            &[
                datastream(json!(5), "pm10", rivm_thing(), Value::Null),
                json!({"observation": observation_at(1, "2026-11-12T09:00:00Z", 1.0, feature), "datastream": 5}),
            ],
        );

        let (records, _) = read(&text);

        assert_eq!(records[0]["readings"]["pm10"]["featureOfInterest"]["feature"], point([5.4797, 51.4433]));
    }

    #[test]
    fn a_feature_of_interest_in_another_encoding_has_no_feature_and_is_counted() {
        let pdf = |id: i64| feature_of_interest(id, "application/pdf", json!("https://h.example/route.pdf"));
        let text = snapshot(
            "range",
            &[
                datastream(json!(5), "pm10", rivm_thing(), Value::Null),
                json!({"observation": observation_at(1, "2026-11-12T08:00:00Z", 1.0, pdf(4711)), "datastream": 5}),
                json!({"observation": observation_at(2, "2026-11-12T09:00:00Z", 2.0, pdf(4712)), "datastream": 5}),
            ],
        );

        let (records, losses) = read(&text);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["readings"]["pm10"]["featureOfInterest"], json!({"id": 4711, "name": "stop 4711"}));
        assert_eq!(
            losses.unreadable_features_of_interest().named(),
            [UnreadableFeatureOfInterest {
                feature: FeatureOfInterestName::Id(EntityId::Integer(4711)),
                encoding: GeometryEncoding::Stated("application/pdf".into()),
            }]
        );
        assert_eq!(losses.unreadable_features_of_interest().total(), 2);
    }

    #[test]
    fn a_snapshot_without_features_of_interest_reads_as_before() {
        let text = snapshot(
            "range",
            &[
                datastream(json!(5), "pm10", rivm_thing(), Value::Null),
                json!({"observation": {"@iot.id": 1, "phenomenonTime": "2026-11-12T09:00:00Z", "result": 1.0, "FeatureOfInterest@iot.navigationLink": "https://api-samenmeten.rivm.nl/v1.0/Observations(1)/FeatureOfInterest"}, "datastream": 5}),
            ],
        );

        let (records, losses) = read(&text);

        assert_eq!(
            records[0]["readings"]["pm10"].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["id", "result", "time", "phenomenonTime", "unit", "datastream", "observedProperty"]
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    /// A `Sensor` with id `id`, called `name`, described as `description`, whose datasheet is the
    /// PDF at `metadata`.
    fn sensor(id: i64, name: &str, description: &str, metadata: &str) -> Value {
        json!({
            "@iot.id": id,
            "@iot.selfLink": format!("https://h.example/v1.1/Sensors({id})"),
            "name": name,
            "description": description,
            "encodingType": "application/pdf",
            "metadata": metadata,
            "Datastreams@iot.navigationLink": format!("https://h.example/v1.1/Sensors({id})/Datastreams"),
        })
    }

    /// Sensor 7, an optical particulate matter sensor.
    fn pm_sensor() -> Value {
        sensor(7, "PM-200", "Optical particulate matter sensor", "https://sensors.example.org/pm-200.pdf")
    }

    /// Sensor 8, an electrochemical nitrogen dioxide sensor.
    fn no2_sensor() -> Value {
        sensor(8, "NO2-50", "Electrochemical nitrogen dioxide sensor", "https://sensors.example.org/no2-50.pdf")
    }

    /// The datastream line `line`, its data stream served with the `Sensor` `sensor`.
    fn with_sensor(mut line: Value, sensor: Value) -> Value {
        line["datastream"]["Sensor"] = sensor;
        line
    }

    #[test]
    fn a_latest_snapshot_gives_each_reading_the_sensor_of_its_data_stream() {
        let text = snapshot(
            "latest",
            &[
                with_sensor(
                    datastream(json!(56776), "pm10", rivm_thing(), json!([observation(1, "2026-11-12T10:00:00Z", 21.4)])),
                    pm_sensor(),
                ),
                with_sensor(
                    datastream(json!(56778), "no2", rivm_thing(), json!([observation(2, "2026-11-12T10:00:00Z", 17.0)])),
                    no2_sensor(),
                ),
            ],
        );

        let (records, losses) = read(&text);

        assert_eq!(
            records[0]["readings"]["pm10"]["sensor"],
            json!({
                "id": 7,
                "name": "PM-200",
                "description": "Optical particulate matter sensor",
                "encodingType": "application/pdf",
                "metadata": "https://sensors.example.org/pm-200.pdf",
            })
        );
        assert_eq!(
            records[0]["readings"]["no2"]["sensor"],
            json!({
                "id": 8,
                "name": "NO2-50",
                "description": "Electrochemical nitrogen dioxide sensor",
                "encodingType": "application/pdf",
                "metadata": "https://sensors.example.org/no2-50.pdf",
            })
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_range_snapshot_gives_every_reading_the_sensor_of_its_data_stream() {
        let text = snapshot(
            "range",
            &[
                with_sensor(datastream(json!(56776), "pm10", rivm_thing(), Value::Null), pm_sensor()),
                with_sensor(datastream(json!(56778), "no2", rivm_thing(), Value::Null), no2_sensor()),
                json!({"observation": observation(1, "2026-11-12T08:00:00Z", 1.0), "datastream": 56776}),
                json!({"observation": observation(2, "2026-11-12T09:00:00Z", 2.0), "datastream": 56776}),
                json!({"observation": observation(3, "2026-11-12T08:00:00Z", 3.0), "datastream": 56778}),
            ],
        );

        let (records, _) = read(&text);

        let sensors: Vec<(&str, &Value)> = records
            .iter()
            .map(|record| {
                let (key, reading) = record["readings"].as_object().unwrap().iter().next().unwrap();
                (key.as_str(), &reading["sensor"]["name"])
            })
            .collect();
        assert_eq!(sensors, [("pm10", &json!("PM-200")), ("pm10", &json!("PM-200")), ("no2", &json!("NO2-50"))]);
    }

    #[test]
    fn a_data_stream_without_a_sensor_gives_readings_without_one() {
        let text = snapshot(
            "latest",
            &[
                with_sensor(
                    datastream(json!(56776), "pm10", rivm_thing(), json!([observation(1, "2026-11-12T10:00:00Z", 21.4)])),
                    pm_sensor(),
                ),
                datastream(json!(56778), "no2", rivm_thing(), json!([observation(2, "2026-11-12T10:00:00Z", 17.0)])),
                with_sensor(
                    datastream(json!(56779), "o3", rivm_thing(), json!([observation(3, "2026-11-12T10:00:00Z", 40.0)])),
                    Value::Null,
                ),
            ],
        );

        let (records, losses) = read(&text);

        assert_eq!(records[0]["readings"]["pm10"]["sensor"]["id"], 7);
        for key in ["no2", "o3"] {
            assert!(!records[0]["readings"][key].as_object().unwrap().contains_key("sensor"), "{key}");
        }
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_snapshot_recorded_without_sensors_reads_as_before() {
        let mut line = datastream(json!(56776), "pm10", rivm_thing(), json!([observation(1, "2026-11-12T10:00:00Z", 21.4)]));
        line["datastream"]["Sensor@iot.navigationLink"] = json!("https://api-samenmeten.rivm.nl/v1.0/Datastreams(56776)/Sensor");

        let (records, losses) = read(&snapshot("latest", &[line]));

        assert_eq!(
            records[0]["readings"]["pm10"].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["id", "result", "time", "phenomenonTime", "unit", "datastream", "observedProperty"]
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_sensor_with_an_unreadable_id_fails_naming_its_line() {
        let text = snapshot(
            "latest",
            &[
                datastream(json!(5), "pm10", rivm_thing(), json!([])),
                with_sensor(datastream(json!(6), "no2", rivm_thing(), json!([])), json!({"@iot.id": 7.5, "name": "NO2-50"})),
            ],
        );

        let error = read_batches(&text, 100).unwrap_err();

        assert!(
            matches!(
                error,
                IngestorError::SensorThings(SensorThingsIngestError::IdUnreadable { line, entity: StaEntity::Sensor }) if line.get() == 3
            ),
            "{error:?}"
        );
        assert!(error.to_string().starts_with("line 3 of the snapshot holds a Sensor without"), "{error}");
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
            IngestorError::SensorThings(SensorThingsIngestError::UnknownDatastream { line, datastream: StreamId::Datastream(EntityId::Integer(7)) }) if line.get() == 3
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

    /// The observation type of a measurement (OGC 18-088 section 8.2.4, Table 12).
    const MEASUREMENT: &str = "http://www.opengis.net/def/observationType/OGC-OM/2.0/OM_Measurement";

    /// A data stream line of the `MultiDatastream` `id` of `thing`, observing each of `properties`, a
    /// name with its unit's symbol, expanded with `observations`.
    fn multi_datastream(id: i64, properties: &[(&str, &str)], thing: &Value, observations: &Value) -> Value {
        json!({"multiDatastream": {
            "@iot.id": id,
            "name": format!("ws-{id}"),
            "description": "Weather station",
            "ObservedProperties": properties.iter().enumerate().map(|(index, (name, _))| json!({"@iot.id": 10 + index, "name": name})).collect::<Vec<_>>(),
            "unitOfMeasurements": properties.iter().map(|(_, symbol)| json!({"symbol": symbol})).collect::<Vec<_>>(),
            "multiObservationDataTypes": properties.iter().map(|_| MEASUREMENT).collect::<Vec<_>>(),
            "Thing": thing,
            "Observations": observations,
        }})
    }

    /// An Observation at `time` of a `MultiDatastream` with the result array `result`.
    fn multi_observation(id: i64, time: &str, result: &Value) -> Value {
        json!({"@iot.id": id, "phenomenonTime": time, "result": result})
    }

    /// The observation line of `observation`, an Observation of the `MultiDatastream` `id`.
    fn multi_observation_line(observation: &Value, id: i64) -> Value {
        json!({"observation": observation, "multiDatastream": id})
    }

    /// The wind `MultiDatastream` 4: wind speed in `m/s` and wind direction in degrees.
    const WIND: [(&str, &str); 2] = [("windSpeed", "m/s"), ("windDirection", "deg")];

    #[test]
    fn a_latest_snapshot_reads_each_element_of_a_multi_datastream_beside_the_datastreams() {
        let text = snapshot(
            "latest",
            &[
                datastream(json!(1), "pm10", rivm_thing(), json!([observation(70, "2026-11-12T09:00:00Z", 18.2)])),
                multi_datastream(
                    4,
                    &WIND,
                    &rivm_thing(),
                    &json!([multi_observation(501, "2026-11-12T09:50:00Z", &json!([4.2, 270]))]),
                ),
            ],
        );

        let (records, losses) = read(&text);

        assert_eq!(records.len(), 1);
        let readings = &records[0]["readings"];
        assert_eq!(readings["pm10"]["result"], json!(18.2));
        assert_eq!(readings["windSpeed"]["result"], json!(4.2));
        assert_eq!(readings["windSpeed"]["unit"]["symbol"], json!("m/s"));
        assert_eq!(readings["windDirection"]["result"], json!(270));
        assert_eq!(readings["windDirection"]["unit"]["symbol"], json!("deg"));
        assert_eq!(
            readings["windSpeed"]["multiDatastream"],
            json!({"id": 4, "name": "ws-4", "description": "Weather station", "observationType": MEASUREMENT, "position": 1})
        );
        assert_eq!(readings["windDirection"]["observedProperty"], json!({"id": 11, "name": "windDirection"}));
        assert_eq!(readings["windDirection"]["id"], json!(501));
        assert!(readings["windSpeed"].get("datastream").is_none());
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_range_snapshot_gives_one_record_per_multi_datastream_observation_with_every_element() {
        let text = snapshot(
            "range",
            &[
                multi_datastream(4, &WIND, &rivm_thing(), &Value::Null),
                multi_observation_line(&multi_observation(501, "2026-11-12T08:00:00Z", &json!([4.2, 270])), 4),
                multi_observation_line(&multi_observation(502, "2026-11-12T09:00:00Z", &json!([5.1, 280])), 4),
                multi_observation_line(&multi_observation(503, "2026-11-12T10:00:00Z", &json!([3.9, 260])), 4),
            ],
        );

        let (records, _) = read(&text);

        let shown: Vec<(Value, Value, Value)> = records
            .iter()
            .map(|record| {
                let readings = &record["readings"];
                (
                    readings["windSpeed"]["id"].clone(),
                    readings["windSpeed"]["result"].clone(),
                    readings["windDirection"]["result"].clone(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                (json!(501), json!(4.2), json!(270)),
                (json!(502), json!(5.1), json!(280)),
                (json!(503), json!(3.9), json!(260))
            ]
        );
    }

    #[test]
    fn a_name_a_datastream_and_an_element_share_is_kept_by_the_datastream() {
        let text = snapshot(
            "latest",
            &[
                multi_datastream(
                    4,
                    &[("temperature", "Cel"), ("humidity", "%")],
                    &rivm_thing(),
                    &json!([multi_observation(501, "2026-11-12T09:50:00Z", &json!([11.5, 80]))]),
                ),
                datastream(json!(9), "temperature", rivm_thing(), json!([observation(90, "2026-11-12T09:55:00Z", 12.0)])),
            ],
        );

        let (records, losses) = read(&text);

        let readings = records[0]["readings"].as_object().unwrap();
        assert_eq!(readings.keys().collect::<Vec<_>>(), ["temperature@4/1", "humidity", "temperature"]);
        assert_eq!(readings["temperature"]["result"], json!(12.0));
        assert_eq!(readings["temperature@4/1"]["result"], json!(11.5));
        assert_eq!(losses.renamed_readings().total(), 1);
        assert_eq!(
            losses.renamed_readings().named()[0].to_string(),
            "Thing 12512 reads element 1 of MultiDatastream 4 as `readings['temperature@4/1']`"
        );
    }

    #[test]
    fn a_result_too_short_gives_no_readings_and_is_counted() {
        let text = snapshot(
            "range",
            &[
                multi_datastream(4, &WIND, &rivm_thing(), &Value::Null),
                multi_observation_line(&multi_observation(501, "2026-11-12T08:00:00Z", &json!([4.2])), 4),
                multi_observation_line(&multi_observation(502, "2026-11-12T09:00:00Z", &json!(4.2)), 4),
            ],
        );

        let (records, losses) = read(&text);

        assert_eq!(records, Vec::<Value>::new());
        assert_eq!(losses.unreadable_multi_results().total(), 2);
        assert_eq!(losses.unreadable_multi_results().named()[0].to_string(), "Observation 501 of MultiDatastream 4");
    }

    #[test]
    fn a_null_element_gives_no_reading_of_it() {
        let text = snapshot(
            "range",
            &[
                multi_datastream(4, &WIND, &rivm_thing(), &Value::Null),
                multi_observation_line(&multi_observation(501, "2026-11-12T08:00:00Z", &json!([null, 270])), 4),
            ],
        );

        let (records, losses) = read(&text);

        assert_eq!(records[0]["readings"].as_object().unwrap().keys().collect::<Vec<_>>(), ["windDirection"]);
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn mismatched_units_fail_the_source_naming_the_multi_datastream() {
        let mut line = multi_datastream(4, &WIND, &rivm_thing(), &Value::Null);
        line["multiDatastream"]["unitOfMeasurements"] = json!([{"symbol": "m/s"}, {"symbol": "deg"}, {"symbol": "K"}]);

        let error = read_batches(&snapshot("range", &[line]), 100).unwrap_err();

        assert!(
            matches!(
                &error,
                IngestorError::SensorThings(SensorThingsIngestError::ElementCountsDiffer { multi_datastream: EntityId::Integer(4), line, .. }) if line.get() == 2
            ),
            "{error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("MultiDatastream 4 with 2 ObservedProperties, 3 `unitOfMeasurements`"),
            "{error}"
        );
    }

    #[test]
    fn a_datastream_and_a_multi_datastream_that_share_an_id_are_two_data_streams() {
        let text = snapshot(
            "range",
            &[
                datastream(json!(4), "pm10", rivm_thing(), Value::Null),
                multi_datastream(4, &WIND, &rivm_thing(), &Value::Null),
                json!({"observation": observation(1, "2026-11-12T08:00:00Z", 18.2), "datastream": 4}),
                multi_observation_line(&multi_observation(2, "2026-11-12T08:00:00Z", &json!([4.2, 270])), 4),
            ],
        );

        let (records, _) = read(&text);

        let keys: Vec<Vec<&String>> = records.iter().map(|record| record["readings"].as_object().unwrap().keys().collect()).collect();
        assert_eq!(keys, [vec!["pm10"], vec!["windSpeed", "windDirection"]]);
    }

    /// The `GeoJSON` Location `id` of a Point at `coordinates`.
    fn geojson_location(id: i64, coordinates: [f64; 2]) -> Value {
        json!({"@iot.id": id, "encodingType": "application/geo+json", "location": {"type": "Point", "coordinates": coordinates}})
    }

    /// The historicalLocation line of `HistoricalLocation` `id` of the Thing `thing`, known at `time`
    /// at `locations`.
    fn historical_location_line(id: i64, time: &str, locations: Value, thing: &Value) -> Value {
        let mut line = json!({"historicalLocation": {"@iot.id": id, "time": time}, "thing": thing});
        line["historicalLocation"]["Locations"] = locations;
        line
    }

    /// A range snapshot of the RIVM Thing's Datastream 56776 with one Observation, followed by
    /// `historical_locations`.
    fn with_historical_locations(historical_locations: &[Value]) -> String {
        let mut lines = vec![
            datastream(json!(56776), "pm10", rivm_thing(), Value::Null),
            json!({"observation": observation(98765, "2026-11-12T09:00:00Z", 21.4), "datastream": 56776}),
        ];
        lines.extend_from_slice(historical_locations);
        snapshot("range", &lines)
    }

    #[test]
    fn each_historical_location_gives_a_record_of_its_thing_after_the_observations_oldest_first() {
        let thing = json!(12512);
        let text = with_historical_locations(&[
            historical_location_line(88, "2026-11-12T08:04:00Z", json!([geojson_location(36190, [5.4797, 51.4433])]), &thing),
            historical_location_line(89, "2026-11-12T08:34:00.250Z", json!([geojson_location(36191, [5.4801, 51.441])]), &thing),
            historical_location_line(90, "2026-11-12T11:04:00+02:00", json!([geojson_location(36192, [5.49, 51.45])]), &thing),
        ]);

        let (records, losses) = read(&text);

        assert_eq!(records.len(), 4);
        assert!(records[0].get("readings").is_some());
        assert!(records[0].get("historicalLocation").is_none());
        let historical = |id: i64, time: &str, coordinates: [f64; 2]| {
            json!({
                "server": {"url": "https://api-samenmeten.rivm.nl/v1.0", "host": "api-samenmeten.rivm.nl", "version": "v1.0"},
                "thing": {"id": 12512, "name": "ILM_I68", "description": "ILM_I68"},
                "location": {"type": "Point", "coordinates": [5.608, 51.313]},
                "locations": [{"id": 36182, "encodingType": "application/vnd.geo+json", "location": {"type": "Point", "coordinates": [5.608, 51.313]}}],
                "historicalLocation": {"id": id, "time": time, "location": {"type": "Point", "coordinates": coordinates}},
            })
        };
        assert_eq!(
            records[1..],
            [
                historical(88, "2026-11-12T08:04:00Z", [5.4797, 51.4433]),
                historical(89, "2026-11-12T08:34:00.250Z", [5.4801, 51.441]),
                historical(90, "2026-11-12T09:04:00Z", [5.49, 51.45]),
            ]
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn a_historical_location_with_a_text_and_a_geojson_location_gives_the_geojson_geometry() {
        let text = with_historical_locations(&[historical_location_line(
            88,
            "2026-11-12T08:04:00Z",
            json!([
                {"@iot.id": 36189, "encodingType": "text/plain", "location": "Stationsplein, platform 2"},
                geojson_location(36190, [5.4797, 51.4433]),
            ]),
            &json!(12512),
        )]);

        let (records, losses) = read(&text);

        assert_eq!(
            records[1]["historicalLocation"]["location"],
            json!({"type": "Point", "coordinates": [5.4797, 51.4433]})
        );
        assert_eq!(losses, SensorThingsLosses::default());
    }

    #[test]
    fn unreadable_historical_locations_give_no_record_and_one_loss_names_the_first() {
        let thing = json!(12512);
        let text = with_historical_locations(&[
            historical_location_line(88, "2026-11-12", json!([geojson_location(36190, [5.4797, 51.4433])]), &thing),
            historical_location_line(
                89,
                "2026-11-12T08:34:00Z",
                json!([{"@iot.id": 36191, "encodingType": "text/plain", "location": "Stationsplein"}]),
                &thing,
            ),
            historical_location_line(90, "2026-11-12T09:04:00Z", json!([geojson_location(36192, [5.49, 51.45])]), &thing),
        ]);

        let (records, losses) = read(&text);

        let ids: Vec<&Value> = records
            .iter()
            .filter_map(|record| record.get("historicalLocation"))
            .map(|historical| &historical["id"])
            .collect();
        assert_eq!(ids, [&json!(90)]);
        let tally = losses.unreadable_historical_locations();
        assert_eq!(
            tally.named(),
            [UnreadableHistoricalLocation {
                historical_location: HistoricalLocationName::Id(EntityId::Integer(88)),
                thing: EntityId::Integer(12512),
                fault: HistoricalLocationFault::Time("2026-11-12".into()),
            }]
        );
        assert_eq!(tally.total(), 2);
    }

    #[test]
    fn an_incremental_snapshot_gives_historical_location_records_without_observations() {
        let thing = json!("bus-12");
        let mut moving = rivm_thing();
        moving["@iot.id"] = thing.clone();
        let text = snapshot(
            "incremental",
            &[
                datastream(json!(56776), "pm10", moving, Value::Null),
                historical_location_line(88, "2026-11-12T08:04:00Z", json!([geojson_location(36190, [5.4797, 51.4433])]), &thing),
            ],
        );

        let (records, _) = read(&text);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["thing"]["id"], thing);
        assert_eq!(records[0]["historicalLocation"]["id"], 88);
        assert!(records[0].get("readings").is_none());
    }

    #[test]
    fn a_historical_location_of_an_unknown_thing_fails_naming_its_line() {
        let text = with_historical_locations(&[historical_location_line(
            88,
            "2026-11-12T08:04:00Z",
            json!([geojson_location(36190, [5.0, 51.0])]),
            &json!(7),
        )]);

        let error = read_batches(&text, 100).unwrap_err();

        assert!(
            matches!(
                error,
                IngestorError::SensorThings(SensorThingsIngestError::UnknownThing { line, thing: EntityId::Integer(7) }) if line.get() == 4
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_data_stream_line_after_a_historical_location_line_fails_naming_the_line() {
        let thing = json!(12512);
        let text = with_historical_locations(&[
            historical_location_line(88, "2026-11-12T08:04:00Z", json!([geojson_location(36190, [5.0, 51.0])]), &thing),
            datastream(json!(56777), "no2", rivm_thing(), Value::Null),
        ]);

        let error = read_batches(&text, 100).unwrap_err();

        assert!(
            matches!(
                error,
                IngestorError::SensorThings(SensorThingsIngestError::Snapshot(SnapshotError::OrderViolation {
                    line,
                    kind: LineKind::Datastream,
                    after: LineKind::HistoricalLocation
                })) if line.get() == 5
            ),
            "{error:?}"
        );
        assert!(error.to_string().contains("line 5 of the snapshot"), "{error}");
    }

    #[test]
    fn a_historical_location_line_in_a_latest_snapshot_fails() {
        let text = snapshot(
            "latest",
            &[
                datastream(json!(5), "pm10", rivm_thing(), json!([])),
                historical_location_line(88, "2026-11-12T08:04:00Z", json!([geojson_location(36190, [5.0, 51.0])]), &json!(12512)),
            ],
        );

        assert!(matches!(
            read_batches(&text, 100),
            Err(IngestorError::SensorThings(SensorThingsIngestError::UnexpectedHistoricalLocation { line })) if line.get() == 3
        ));
    }
}
