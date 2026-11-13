use crate::sensorthings::{
    error::{SensorThingsIngestError, StaEntity},
    ingest_losses::{SensorThingsLosses, UnreadableLocation},
    location::{ThingLocation, thing_location},
    null_members::{move_member, present},
    reading_keys::{ReadingKey, RenamedReading, reading_keys},
    served_id::{optional_served_id, served_id},
};
use cassiopeia_sensorthings::{
    entity_id::{EntityId, ID_MEMBER},
    snapshot::{line::DatastreamLine, line_number::LineNumber},
};
use indexmap::{IndexMap, map::Entry};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// The Datastream members a reading's `datastream` carries.
const DATASTREAM_MEMBERS: [&str; 4] = ["name", "description", "observationType", "properties"];

/// The `ObservedProperty` members a reading's `observedProperty` carries.
const OBSERVED_PROPERTY_MEMBERS: [&str; 4] = ["name", "definition", "description", "properties"];

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
    /// The key its readings are found under in a record's `readings`.
    pub(super) key: ReadingKey,
    /// The reading's `unit`: the data stream's `unitOfMeasurement`, when it has one.
    pub(super) unit: Option<Value>,
    /// The reading's `datastream`: the data stream's id and members.
    pub(super) members: Map<String, Value>,
    /// The reading's `observedProperty`: the `ObservedProperty`'s id and members.
    pub(super) observed_property: Map<String, Value>,
    /// The newest Observation a latest read expanded into the datastream line, with that line.
    pub(super) latest: Option<(Map<String, Value>, LineNumber)>,
}

/// The snapshot's Things and data streams, built from its datastream lines.
pub(super) struct ThingIndex {
    /// The Things, in the order the snapshot first names them.
    things: Vec<IndexedThing>,
    /// Where each data stream is: its Thing's position and its own position under that Thing.
    positions: HashMap<EntityId, (usize, usize)>,
}

/// Collects the datastream lines of a snapshot into a [`ThingIndex`].
#[derive(Default)]
pub(super) struct ThingIndexBuilder {
    /// The Things seen so far, in the order the snapshot first names them.
    things: IndexMap<EntityId, PendingThing>,
    /// The ids of the data streams seen so far.
    seen: HashSet<EntityId>,
}

/// A Thing whose data streams are still being collected.
struct PendingThing {
    members: Map<String, Value>,
    location: Option<Value>,
    datastreams: Vec<PendingDatastream>,
}

/// A data stream whose reading key is not yet known, since it depends on the Thing's other data
/// streams.
struct PendingDatastream {
    id: EntityId,
    name: Box<str>,
    unit: Option<Value>,
    members: Map<String, Value>,
    observed_property: Map<String, Value>,
    latest: Option<(Map<String, Value>, LineNumber)>,
}

impl ThingIndexBuilder {
    /// Adds the data stream of the datastream line `line`, read as line `number`, counting the
    /// Thing's unreadable Location the first time the Thing is seen.
    ///
    /// A data stream seen before keeps its first line, so a server that pages one twice gives it one
    /// set of readings.
    ///
    /// # Errors
    ///
    /// Returns [`SensorThingsIngestError`] when the data stream, its Thing or a Location has no
    /// readable id, or the data stream has no Thing or no named `ObservedProperty`.
    pub(super) fn add(&mut self, line: DatastreamLine, number: LineNumber, losses: &mut SensorThingsLosses) -> Result<(), SensorThingsIngestError> {
        let mut served = line.into_datastream();
        let id = served_id(&served, number, StaEntity::Datastream)?;
        if self.seen.contains(&id) {
            return Ok(());
        }
        let Some(Value::Object(mut thing)) = served.remove("Thing") else {
            return Err(SensorThingsIngestError::ThingMissing { line: number, datastream: id });
        };
        let thing_id = served_id(&thing, number, StaEntity::Thing)?;
        let Some(Value::Object(observed)) = served.remove("ObservedProperty") else {
            return Err(SensorThingsIngestError::ObservedPropertyMissing { datastream: id });
        };
        let Some(name) = observed.get("name").and_then(Value::as_str).map(Box::<str>::from) else {
            return Err(SensorThingsIngestError::ObservedPropertyUnnamed { datastream: id });
        };
        let observed_id = optional_served_id(&observed, number, StaEntity::ObservedProperty)?;
        let latest = newest_observation(served.remove("Observations")).map(|observation| (observation, number));
        let datastream = PendingDatastream {
            unit: served.remove("unitOfMeasurement").and_then(present),
            members: record_members(served, Some(&id), &DATASTREAM_MEMBERS),
            observed_property: record_members(observed, observed_id.as_ref(), &OBSERVED_PROPERTY_MEMBERS),
            name,
            latest,
            // The set keeps the id to skip a repeated line, while the data stream keeps it to be
            // found by; an id is a small value.
            id: id.clone(),
        };
        self.seen.insert(id);

        match self.things.entry(thing_id) {
            Entry::Occupied(entry) => entry.into_mut().datastreams.push(datastream),
            Entry::Vacant(entry) => {
                let location = match thing_location(thing.remove("Locations"), number)? {
                    ThingLocation::Geometry(geometry) => Some(geometry),
                    ThingLocation::Unreadable(encoding) => {
                        losses.unreadable_location(UnreadableLocation {
                            // The loss is reported after the Thing's own id has gone into its record.
                            thing: entry.key().clone(),
                            encoding,
                        });
                        None
                    }
                    ThingLocation::Absent => None,
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

    /// Gives every data stream its reading key and returns the index, counting each data stream
    /// whose key is not its `ObservedProperty`'s name.
    pub(super) fn finish(self, losses: &mut SensorThingsLosses) -> ThingIndex {
        let mut positions = HashMap::new();
        let things = self
            .things
            .into_iter()
            .enumerate()
            .map(|(thing_position, (thing_id, pending))| {
                let named: Vec<(&EntityId, &str)> = pending.datastreams.iter().map(|datastream| (&datastream.id, &*datastream.name)).collect();
                let keys = reading_keys(&named);
                let datastreams = pending
                    .datastreams
                    .into_iter()
                    .zip(keys)
                    .enumerate()
                    .map(|(position, (datastream, key))| {
                        if key.as_str() != &*datastream.name {
                            losses.rename(RenamedReading {
                                // The rename is reported after the Thing and the data stream have
                                // gone into the index; ids are small values.
                                thing: thing_id.clone(),
                                datastream: datastream.id.clone(),
                                key: key.clone(),
                            });
                        }
                        positions.insert(datastream.id, (thing_position, position));
                        IndexedDatastream {
                            key,
                            unit: datastream.unit,
                            members: datastream.members,
                            observed_property: datastream.observed_property,
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
    pub(super) fn datastream(&self, id: &EntityId) -> Option<(&IndexedThing, &IndexedDatastream)> {
        let &(thing, datastream) = self.positions.get(id)?;
        let thing = self.things.get(thing)?;
        Some((thing, thing.datastreams.get(datastream)?))
    }
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

/// The members a record carries of a served entity: its `id`, then each of `keys` it has.
fn record_members(mut served: Map<String, Value>, id: Option<&EntityId>, keys: &[&str]) -> Map<String, Value> {
    let mut members = Map::new();
    if let Some(id) = id {
        members.insert("id".to_owned(), id.json());
    }
    served.remove(ID_MEMBER);
    for key in keys {
        move_member(&mut members, key, &mut served, key);
    }
    members
}
