use crate::{
    entity_id::EntityId,
    multi_datastream_elements::{ElementCounts, take_elements},
    observed_property_name::ObservedPropertyName,
    offered_readings::{error::OfferedReadingError, offered_reading::OfferedReading, unit_resolution::UnitResolution},
    stream_id::StreamId,
    stream_kind::StreamKind,
    unit_of_measurement::UnitOfMeasurement,
};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// The readings a server offers, gathered one served data stream at a time from discoveries that
/// expand each data stream's Thing and what it observes.
#[derive(Debug, Default)]
pub struct ReadingCatalog {
    /// What is known of each reading so far, by name, so the readings come out sorted by name.
    readings: BTreeMap<ObservedPropertyName, ReadingStreams>,
    /// The data streams added so far.
    seen: HashSet<StreamId>,
}

/// What is known so far of the data streams of one reading.
#[derive(Debug, Default)]
struct ReadingStreams {
    /// The distinct texts their units are recognised by.
    units: BTreeSet<Box<str>>,
    /// What `cefact_unit` makes of each of their units.
    codes: BTreeSet<UnitResolution>,
    /// How many Datastreams of the name there are.
    datastreams: usize,
    /// How many `MultiDatastreams` have an element of the name.
    multi_datastreams: usize,
    /// How many readings of the name each Thing holds.
    things: HashMap<EntityId, usize>,
}

/// One reading a served data stream gives: the `ObservedProperty` it is named after and its unit,
/// as served.
struct ServedReading {
    /// The `ObservedProperty`, as served.
    observed_property: Option<Value>,
    /// The unit of measurement, as served.
    unit: Option<Value>,
}

impl ReadingCatalog {
    /// Adds the data stream of kind `kind` the server served as `served`, with its Thing and what it
    /// observes expanded: a Datastream gives one reading, named after its `ObservedProperty`, and a
    /// `MultiDatastream` one per element, named after the `ObservedProperty` at its position.
    ///
    /// A data stream added before is not counted again, so a server that pages one twice lists it
    /// once, as a run reads it once.
    ///
    /// # Errors
    ///
    /// Returns the [`OfferedReadingError`] that names what the data stream lacks: a readable id, a
    /// Thing with a readable id, an `ObservedProperty` with a `name` for each reading, or, for a
    /// `MultiDatastream`, as many units of measurement and observation types as `ObservedProperties`.
    pub fn add(&mut self, kind: StreamKind, mut served: Map<String, Value>) -> Result<(), OfferedReadingError> {
        let id = StreamId::of(kind, &served).ok_or(OfferedReadingError::IdUnreadable(kind))?;
        if self.seen.contains(&id) {
            return Ok(());
        }
        let Some(thing) = object(&served, "Thing").and_then(EntityId::of) else {
            return Err(OfferedReadingError::ThingUnreadable { datastream: id });
        };
        let readings = match served_readings(kind, &mut served) {
            Ok(readings) => readings,
            Err(counts) => return Err(OfferedReadingError::ElementCountsDiffer { datastream: id, counts }),
        };
        let mut named = Vec::with_capacity(readings.len());
        for reading in &readings {
            let Some(name) = reading
                .observed_property
                .as_ref()
                .and_then(|observed| observed.get("name"))
                .and_then(Value::as_str)
            else {
                return Err(OfferedReadingError::ObservedPropertyUnnamed { datastream: id });
            };
            named.push((name, reading.unit.as_ref().and_then(Value::as_object)));
        }
        if named.is_empty() {
            return Err(OfferedReadingError::ObservedPropertyUnnamed { datastream: id });
        }

        let mut counted: HashSet<&str> = HashSet::new();
        for (name, unit) in named {
            let unit = unit
                .map(|unit| UnitOfMeasurement::read(|member| unit.get(member).and_then(Value::as_str)))
                .unwrap_or_default();
            let streams = self.readings.entry(ObservedPropertyName::from(Box::<str>::from(name))).or_default();
            // A data stream is counted once per name, however many of its elements share it.
            if counted.insert(name) {
                streams.count(kind);
            }
            // Each reading counts towards its Thing, which keeps its id; an id is a small value.
            streams.add(thing.clone(), unit);
        }
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
    /// Counts one more data stream of kind `kind` with a reading of the name.
    const fn count(&mut self, kind: StreamKind) {
        match kind {
            StreamKind::Datastream => self.datastreams += 1,
            StreamKind::MultiDatastream => self.multi_datastreams += 1,
        }
    }

    /// Counts one more reading, held by `thing`, whose unit is `unit`.
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
            .datastreams(self.datastreams)
            .multi_datastreams(self.multi_datastreams)
            .things_with_repeats(self.things.values().filter(|&&held| held > 1).count())
            .units(self.units)
            .codes(self.codes)
            .build()
    }
}

/// The readings the served data stream `served` of kind `kind` gives, taken out of it: its
/// `ObservedProperty` and `unitOfMeasurement` for a Datastream, and for a `MultiDatastream` the
/// `ObservedProperty` and the unit of measurement of each element.
///
/// # Errors
///
/// Returns the [`ElementCounts`] of a `MultiDatastream` whose members differ in number.
fn served_readings(kind: StreamKind, served: &mut Map<String, Value>) -> Result<Vec<ServedReading>, ElementCounts> {
    match kind {
        StreamKind::Datastream => Ok(vec![ServedReading {
            observed_property: served.remove("ObservedProperty"),
            unit: served.remove("unitOfMeasurement"),
        }]),
        StreamKind::MultiDatastream => Ok(take_elements(served)?
            .into_iter()
            .map(|element| ServedReading {
                observed_property: Some(element.observed_property),
                unit: Some(element.unit),
            })
            .collect()),
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
        stream_id::StreamId,
        stream_kind::StreamKind,
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
            catalog.add(StreamKind::Datastream, datastream.clone()).unwrap();
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

        assert_eq!(
            catalog.add(StreamKind::Datastream, without_id),
            Err(OfferedReadingError::IdUnreadable(StreamKind::Datastream))
        );
        assert_eq!(
            catalog.add(StreamKind::Datastream, without_thing),
            Err(OfferedReadingError::ThingUnreadable {
                datastream: StreamId::Datastream(EntityId::Integer(2))
            })
        );
        assert_eq!(
            catalog.add(StreamKind::Datastream, unnamed),
            Err(OfferedReadingError::ObservedPropertyUnnamed {
                datastream: StreamId::Datastream(EntityId::Integer(3))
            })
        );
        assert_eq!(catalog.into_readings(), []);
    }

    /// A served `MultiDatastream` `id` of the Thing `thing`, whose elements are named `names`, each
    /// with the unit of the same position of `units`.
    fn multi_datastream(id: i64, thing: i64, names: &[&str], units: &[Value]) -> Map<String, Value> {
        let Value::Object(members) = json!({
            "@iot.id": id,
            "name": format!("multi stream {id}"),
            "Thing": {"@iot.id": thing, "Locations": []},
            "ObservedProperties": names.iter().map(|name| json!({"name": name})).collect::<Vec<_>>(),
            "unitOfMeasurements": units,
            "multiObservationDataTypes": names.iter().map(|_| json!("OM_Measurement")).collect::<Vec<_>>(),
        }) else {
            panic!("expected an object");
        };
        members
    }

    #[test]
    fn each_element_of_a_multi_datastream_is_a_reading_with_the_unit_at_its_position() {
        let mut catalog = ReadingCatalog::default();
        catalog
            .add(StreamKind::Datastream, datastream(1, 1, "pm10", &json!({"symbol": "ug/m3"})))
            .unwrap();
        catalog
            .add(
                StreamKind::MultiDatastream,
                multi_datastream(4, 1, &["windSpeed", "windDirection"], &[json!({"symbol": "m/s"}), json!({"symbol": "deg"})]),
            )
            .unwrap();

        let listed = catalog.into_readings();

        let rows: Vec<(String, Vec<String>, usize, usize)> = listed
            .iter()
            .map(|reading| {
                (
                    reading.name().to_string(),
                    reading.units().iter().map(ToString::to_string).collect(),
                    reading.datastreams(),
                    reading.multi_datastreams(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("pm10".to_owned(), vec!["ug/m3".to_owned()], 1, 0),
                ("windDirection".to_owned(), vec!["deg".to_owned()], 0, 1),
                ("windSpeed".to_owned(), vec!["m/s".to_owned()], 0, 1),
            ]
        );
    }

    #[test]
    fn a_name_a_datastream_and_an_element_of_one_thing_share_is_a_repeat() {
        let mut catalog = ReadingCatalog::default();
        catalog
            .add(StreamKind::Datastream, datastream(4, 1, "temperature", &json!({"symbol": "Cel"})))
            .unwrap();
        catalog
            .add(
                StreamKind::MultiDatastream,
                multi_datastream(4, 1, &["temperature", "temperature"], &[json!({"symbol": "Cel"}), json!({"symbol": "Cel"})]),
            )
            .unwrap();

        let listed = catalog.into_readings();

        assert_eq!(
            (listed[0].datastreams(), listed[0].multi_datastreams(), listed[0].things_with_repeats()),
            (1, 1, 1)
        );
    }

    #[test]
    fn a_multi_datastream_whose_members_differ_in_number_or_without_properties_is_refused() {
        let mut catalog = ReadingCatalog::default();
        let mismatched = multi_datastream(4, 1, &["windSpeed", "windDirection"], &[json!({}), json!({}), json!({})]);
        let empty = multi_datastream(5, 1, &[], &[]);

        assert!(matches!(
            catalog.add(StreamKind::MultiDatastream, mismatched),
            Err(OfferedReadingError::ElementCountsDiffer { datastream, .. }) if datastream == StreamId::MultiDatastream(EntityId::Integer(4))
        ));
        assert_eq!(
            catalog.add(StreamKind::MultiDatastream, empty),
            Err(OfferedReadingError::ObservedPropertyUnnamed {
                datastream: StreamId::MultiDatastream(EntityId::Integer(5))
            })
        );
    }
}
