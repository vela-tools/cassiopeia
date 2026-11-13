use crate::{
    entity_id::EntityId,
    observed_property_name::ObservedPropertyName,
    offered_readings::{error::OfferedReadingError, offered_reading::OfferedReading, unit_resolution::UnitResolution},
    unit_of_measurement::UnitOfMeasurement,
};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// The readings a server offers, gathered one served Datastream at a time from a discovery that
/// expands each Datastream's Thing and `ObservedProperty`.
#[derive(Debug, Default)]
pub struct ReadingCatalog {
    /// What is known of each reading so far, by name, so the readings come out sorted by name.
    readings: BTreeMap<ObservedPropertyName, ReadingStreams>,
    /// The ids of the data streams added so far.
    seen: HashSet<EntityId>,
}

/// What is known so far of the data streams of one reading.
#[derive(Debug, Default)]
struct ReadingStreams {
    /// The distinct texts their units are recognised by.
    units: BTreeSet<Box<str>>,
    /// What `cefact_unit` makes of each of their units.
    codes: BTreeSet<UnitResolution>,
    /// How many of them each Thing holds.
    things: HashMap<EntityId, usize>,
}

impl ReadingCatalog {
    /// Adds the Datastream `datastream` as the server served it, with its Thing and its
    /// `ObservedProperty` expanded.
    ///
    /// A data stream added before is not counted again, so a server that pages one twice lists it
    /// once, as a run reads it once.
    ///
    /// # Errors
    ///
    /// Returns the [`OfferedReadingError`] that names what the Datastream lacks: a readable id, a
    /// Thing with a readable id, or an `ObservedProperty` with a `name`.
    pub fn add(&mut self, datastream: &Map<String, Value>) -> Result<(), OfferedReadingError> {
        let id = EntityId::of(datastream).ok_or(OfferedReadingError::DatastreamIdUnreadable)?;
        if self.seen.contains(&id) {
            return Ok(());
        }
        let Some(thing) = object(datastream, "Thing").and_then(EntityId::of) else {
            return Err(OfferedReadingError::ThingUnreadable { datastream: id });
        };
        let Some(name) = object(datastream, "ObservedProperty")
            .and_then(|observed| observed.get("name"))
            .and_then(Value::as_str)
        else {
            return Err(OfferedReadingError::ObservedPropertyUnnamed { datastream: id });
        };
        let unit = object(datastream, "unitOfMeasurement")
            .map(|unit| UnitOfMeasurement::read(|member| unit.get(member).and_then(Value::as_str)))
            .unwrap_or_default();

        self.readings
            .entry(ObservedPropertyName::from(Box::<str>::from(name)))
            .or_default()
            .add(thing, unit);
        self.seen.insert(id);
        Ok(())
    }

    /// The readings added, sorted by name.
    #[must_use]
    pub fn into_readings(self) -> Vec<OfferedReading> {
        self.readings.into_iter().map(|(name, streams)| streams.into_reading(name)).collect()
    }
}

impl ReadingStreams {
    /// Counts one more data stream, held by `thing`, whose unit is `unit`.
    fn add(&mut self, thing: EntityId, unit: UnitOfMeasurement<'_>) {
        if let Some(text) = unit.text() {
            if !self.units.contains(text) {
                self.units.insert(Box::from(text));
            }
            self.codes.insert(unit.code().map_or(UnitResolution::Unknown, UnitResolution::Code));
        }
        *self.things.entry(thing).or_default() += 1;
    }

    /// The reading `name` these data streams make up.
    fn into_reading(self, name: ObservedPropertyName) -> OfferedReading {
        OfferedReading::builder()
            .name(name)
            .datastreams(self.things.values().sum())
            .things_with_repeats(self.things.values().filter(|&&held| held > 1).count())
            .units(self.units)
            .codes(self.codes)
            .build()
    }
}

/// The member `name` of `entity`, when it is a JSON object.
fn object<'a>(entity: &'a Map<String, Value>, name: &str) -> Option<&'a Map<String, Value>> {
    entity.get(name).and_then(Value::as_object)
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_id::EntityId,
        offered_readings::{error::OfferedReadingError, reading_catalog::ReadingCatalog, unit_resolution::UnitResolution},
    };
    use cefact_units::UnitCode;
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    /// A served Datastream `id` of the Thing `thing`, whose readings are named `name`, with the
    /// unit `unit`.
    fn datastream(id: i64, thing: i64, name: &str, unit: &Value) -> Map<String, Value> {
        let Value::Object(members) = json!({
            "@iot.id": id,
            "name": format!("stream {id}"),
            "unitOfMeasurement": unit,
            "Thing": {"@iot.id": thing, "Locations": []},
            "ObservedProperty": {"@iot.id": 1, "name": name},
        }) else {
            panic!("expected an object");
        };
        members
    }

    fn catalog(datastreams: &[Map<String, Value>]) -> ReadingCatalog {
        let mut catalog = ReadingCatalog::default();
        for datastream in datastreams {
            catalog.add(datastream).unwrap();
        }
        catalog
    }

    /// Each reading as its name, data stream count and count of Things with repeats.
    fn counts(catalog: ReadingCatalog) -> Vec<(String, usize, usize)> {
        catalog
            .into_readings()
            .iter()
            .map(|reading| (reading.name().to_string(), reading.datastreams(), reading.things_with_repeats()))
            .collect()
    }

    #[test]
    fn readings_are_listed_by_name_with_their_data_streams() {
        let ug = json!({"symbol": "ug/m3"});
        let listed = catalog(&[datastream(1, 1, "pm10", &ug), datastream(2, 1, "no2", &ug), datastream(3, 2, "pm10", &ug)]);

        assert_eq!(counts(listed), [("no2".to_owned(), 1, 0), ("pm10".to_owned(), 2, 0)]);
    }

    #[test]
    fn a_thing_with_two_data_streams_of_a_name_counts_as_one_with_repeats() {
        let ug = json!({"symbol": "ug/m3"});
        let listed = catalog(&[
            datastream(1, 7, "pm10", &ug),
            datastream(2, 7, "pm10", &ug),
            datastream(3, 7, "pm10", &ug),
            datastream(4, 8, "pm10", &ug),
        ]);

        assert_eq!(counts(listed), [("pm10".to_owned(), 4, 1)]);
    }

    #[test]
    fn a_data_stream_served_twice_is_counted_once() {
        let ug = json!({"symbol": "ug/m3"});
        let listed = catalog(&[datastream(1, 7, "pm10", &ug), datastream(1, 7, "pm10", &ug)]);

        assert_eq!(counts(listed), [("pm10".to_owned(), 1, 0)]);
    }

    #[test]
    fn units_are_listed_by_their_text_with_what_cefact_unit_makes_of_them() {
        let listed = catalog(&[
            datastream(
                1,
                1,
                "pm10",
                &json!({"name": "ug/m3", "symbol": "ug/m3", "definition": "https://qudt.org/vocab/unit/MicroGM-PER-M3"}),
            ),
            datastream(2, 2, "pm10", &json!({"symbol": "\u{b5}g/m\u{b3}"})),
            datastream(3, 3, "pm10", &json!({"name": "xyzzy"})),
        ])
        .into_readings();

        let units: Vec<&str> = listed[0].units().iter().map(AsRef::as_ref).collect();
        assert_eq!(units, ["ug/m3", "xyzzy", "\u{b5}g/m\u{b3}"]);
        assert_eq!(
            listed[0].codes().iter().copied().collect::<Vec<_>>(),
            [UnitResolution::Code(UnitCode::Gq), UnitResolution::Unknown]
        );
    }

    #[test]
    fn a_unit_without_text_lists_no_unit_and_no_code() {
        let listed = catalog(&[
            datastream(1, 1, "status", &json!({"name": null, "symbol": null, "definition": null})),
            datastream(2, 2, "status", &Value::Null),
        ])
        .into_readings();

        assert!(listed[0].units().is_empty());
        assert!(listed[0].codes().is_empty());
        assert_eq!(listed[0].datastreams(), 2);
    }

    #[test]
    fn a_data_stream_lacking_an_id_a_thing_or_a_named_observed_property_is_refused() {
        let mut catalog = ReadingCatalog::default();
        let mut without_id = datastream(1, 1, "pm10", &Value::Null);
        without_id.remove("@iot.id");
        let mut without_thing = datastream(2, 1, "pm10", &Value::Null);
        without_thing.remove("Thing");
        let mut unnamed = datastream(3, 1, "pm10", &Value::Null);
        unnamed.insert("ObservedProperty".to_owned(), json!({"@iot.id": 1}));

        assert_eq!(catalog.add(&without_id), Err(OfferedReadingError::DatastreamIdUnreadable));
        assert_eq!(
            catalog.add(&without_thing),
            Err(OfferedReadingError::ThingUnreadable {
                datastream: EntityId::Integer(2)
            })
        );
        assert_eq!(
            catalog.add(&unnamed),
            Err(OfferedReadingError::ObservedPropertyUnnamed {
                datastream: EntityId::Integer(3)
            })
        );
        assert_eq!(catalog.into_readings(), []);
    }
}
