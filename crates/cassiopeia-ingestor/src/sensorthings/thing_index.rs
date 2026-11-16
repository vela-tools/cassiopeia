use crate::sensorthings::{
    encoded_geometry::EncodedGeometry,
    error::{SensorThingsIngestError, StaEntity},
    ingest_losses::{SensorThingsLosses, UnreadableLocation},
    location::thing_location,
    reading_keys::{ReadingKey, ReadingSource, RenamedReading, reading_keys},
    record_builder::{DatastreamParts, record_members},
    sensor::sensor,
    served_id::served_id,
    stream_readings::{StreamReading, stream_readings},
};
use cassiopeia_sensorthings::{
    entity_id::EntityId,
    snapshot::{line::DatastreamLine, line_number::LineNumber},
    stream_id::StreamId,
    stream_kind::StreamKind,
};
use indexmap::{IndexMap, map::Entry};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// The Thing members a record's `thing` carries.
const THING_MEMBERS: [&str; 3] = ["name", "description", "properties"];

/// A Thing of the snapshot with its data streams, as every record of it needs them.
pub(super) struct IndexedThing {
    /// The record's `thing`: the Thing's id and members.
    pub(super) members: Map<String, Value>,
    /// The record's `location`, when the Thing has a `GeoJSON` one.
    pub(super) location: Option<Value>,
    /// The Thing's data streams, in snapshot order.
    pub(super) datastreams: Vec<IndexedDatastream>,
}

/// A data stream of the snapshot, as each of its readings needs it.
pub(super) struct IndexedDatastream {
    /// The data stream.
    pub(super) id: StreamId,
    /// The readings it gives each of its Observations: one for a Datastream, and one per element for
    /// a `MultiDatastream`, in position order.
    pub(super) readings: Vec<IndexedReading>,
    /// The newest Observation a latest read expanded into the data stream line, with that line.
    pub(super) latest: Option<(Map<String, Value>, LineNumber)>,
}

/// One reading a data stream gives each of its Observations, as a record holds it.
pub(super) struct IndexedReading {
    /// The key the reading is found under in a record's `readings`.
    pub(super) key: ReadingKey,
    /// What the reading carries of its data stream.
    pub(super) parts: DatastreamParts,
}

/// The snapshot's Things and data streams, built from its data stream lines.
pub(super) struct ThingIndex {
    /// The Things, in the order the snapshot first names them.
    things: Vec<IndexedThing>,
    /// Where each data stream is: its Thing's position and its own position under that Thing.
    positions: HashMap<StreamId, (usize, usize)>,
}

/// Collects the data stream lines of a snapshot into a [`ThingIndex`].
#[derive(Default)]
pub(super) struct ThingIndexBuilder {
    /// The Things seen so far, in the order the snapshot first names them.
    things: IndexMap<EntityId, PendingThing>,
    /// The data streams seen so far.
    seen: HashSet<StreamId>,
}

/// A Thing whose data streams are still being collected.
struct PendingThing {
    members: Map<String, Value>,
    location: Option<Value>,
    datastreams: Vec<PendingDatastream>,
}

/// A data stream whose reading keys are not yet known, since they depend on the Thing's other data
/// streams.
struct PendingDatastream {
    id: StreamId,
    readings: Vec<StreamReading>,
    latest: Option<(Map<String, Value>, LineNumber)>,
}

impl ThingIndexBuilder {
    /// Adds the data stream of the data stream line `line`, read as line `number`, counting the
    /// Thing's unreadable Location the first time the Thing is seen.
    ///
    /// A data stream seen before keeps its first line, so a server that pages one twice gives it one
    /// set of readings. A Datastream and a `MultiDatastream` that share an id are two data streams.
    ///
    /// # Errors
    ///
    /// Returns [`SensorThingsIngestError`] when the data stream, its Thing or a Location has no
    /// readable id, an `ObservedProperty` or its `Sensor` has an id that is neither an integer nor a
    /// text, the data stream has no Thing or no named `ObservedProperty`, or a `MultiDatastream`'s
    /// `ObservedProperties`, `unitOfMeasurements` and `multiObservationDataTypes` differ in number.
    pub(super) fn add(&mut self, line: DatastreamLine, number: LineNumber, losses: &mut SensorThingsLosses) -> Result<(), SensorThingsIngestError> {
        let (kind, mut served) = line.into_parts();
        let entity = match kind {
            StreamKind::Datastream => StaEntity::Datastream,
            StreamKind::MultiDatastream => StaEntity::MultiDatastream,
        };
        let id = StreamId::new(kind, served_id(&served, number, entity)?);
        if self.seen.contains(&id) {
            return Ok(());
        }
        let Some(Value::Object(mut thing)) = served.remove("Thing") else {
            return Err(SensorThingsIngestError::ThingMissing { line: number, datastream: id });
        };
        let thing_id = served_id(&thing, number, StaEntity::Thing)?;
        let sensor = sensor(&mut served, number)?;
        let latest = newest_observation(served.remove("Observations")).map(|observation| (observation, number));
        let datastream = PendingDatastream {
            readings: stream_readings(&id, served, sensor, number)?,
            latest,
            // The set keeps the data stream to skip a repeated line, while the index keeps it to be
            // found by; a data stream's id is a small value.
            id: id.clone(),
        };
        self.seen.insert(id);

        match self.things.entry(thing_id) {
            Entry::Occupied(entry) => entry.into_mut().datastreams.push(datastream),
            Entry::Vacant(entry) => {
                let location = match thing_location(thing.remove("Locations"), number)? {
                    EncodedGeometry::Geometry(geometry) => Some(geometry),
                    EncodedGeometry::Unreadable(encoding) => {
                        losses.unreadable_location(UnreadableLocation {
                            // The loss is reported after the Thing's own id has gone into its record.
                            thing: entry.key().clone(),
                            encoding,
                        });
                        None
                    }
                    EncodedGeometry::Absent => None,
                };
                let members = record_members(thing, Some(entry.key()), &THING_MEMBERS);
                entry.insert(PendingThing {
                    members,
                    location,
                    datastreams: vec![datastream],
                });
            }
        }
        Ok(())
    }

    /// Gives every reading its key and returns the index, counting each reading whose key is not its
    /// `ObservedProperty`'s name.
    pub(super) fn finish(self, losses: &mut SensorThingsLosses) -> ThingIndex {
        let mut positions = HashMap::new();
        let things = self
            .things
            .into_iter()
            .enumerate()
            .map(|(thing_position, (thing_id, pending))| {
                let keys = thing_keys(&pending.datastreams);
                let mut keys = keys.into_iter();
                let datastreams = pending
                    .datastreams
                    .into_iter()
                    .enumerate()
                    .map(|(position, datastream)| {
                        let readings = datastream
                            .readings
                            .into_iter()
                            .zip(keys.by_ref())
                            .map(|(reading, key)| {
                                if key.as_str() != &*reading.name {
                                    losses.rename(RenamedReading {
                                        // The rename is reported after the Thing has gone into the
                                        // index; an id is a small value.
                                        thing: thing_id.clone(),
                                        source: reading.source,
                                        // The rename names the key the index keeps.
                                        key: key.clone(),
                                    });
                                }
                                IndexedReading { key, parts: reading.parts }
                            })
                            .collect();
                        // The map finds the data stream the index keeps; its id is a small value.
                        positions.insert(datastream.id.clone(), (thing_position, position));
                        IndexedDatastream {
                            id: datastream.id,
                            readings,
                            latest: datastream.latest,
                        }
                    })
                    .collect();
                IndexedThing {
                    members: pending.members,
                    location: pending.location,
                    datastreams,
                }
            })
            .collect();
        ThingIndex { things, positions }
    }
}

impl ThingIndex {
    /// The Things, in the order the snapshot first names them.
    pub(super) fn into_things(self) -> Vec<IndexedThing> {
        self.things
    }

    /// The data stream `id` and the Thing it belongs to.
    pub(super) fn datastream(&self, id: &StreamId) -> Option<(&IndexedThing, &IndexedDatastream)> {
        let &(thing, datastream) = self.positions.get(id)?;
        let thing = self.things.get(thing)?;
        Some((thing, thing.datastreams.get(datastream)?))
    }
}

/// The keys of the readings of one Thing's data streams, in the order of the data streams and of
/// their readings.
fn thing_keys(datastreams: &[PendingDatastream]) -> Vec<ReadingKey> {
    let named: Vec<(&ReadingSource, &str)> = datastreams
        .iter()
        .flat_map(|datastream| datastream.readings.iter().map(|reading| (&reading.source, &*reading.name)))
        .collect();
    reading_keys(&named)
}

/// The first Observation a latest read expanded, the newest, since it asks for them newest first.
fn newest_observation(observations: Option<Value>) -> Option<Map<String, Value>> {
    let Some(Value::Array(observations)) = observations else {
        return None;
    };
    match observations.into_iter().next() {
        Some(Value::Object(observation)) => Some(observation),
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) | None => None,
    }
}
