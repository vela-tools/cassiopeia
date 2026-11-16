use derive_more::Display;
use serde_json::{Map, Value};

/// The member a `MultiDatastream` is served with its expanded `ObservedProperties` in.
const OBSERVED_PROPERTIES: &str = "ObservedProperties";

/// The member that holds a `MultiDatastream`'s unit of measurement for each value of a result.
const UNITS: &str = "unitOfMeasurements";

/// The member that holds a `MultiDatastream`'s observation type for each value of a result.
const OBSERVATION_TYPES: &str = "multiObservationDataTypes";

/// One element of a `MultiDatastream`: what the values at one position of its Observations'
/// results share.
///
/// The `MultiDatastream` extension (OGC 18-088 section 12) gives each Observation of a
/// `MultiDatastream` one `result` array, whose values correspond, in order, to the
/// `MultiDatastream`'s `ObservedProperties`, to its `unitOfMeasurements` and to its
/// `multiObservationDataTypes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiDatastreamElement {
    /// The `ObservedProperty` at the element's position, as served.
    pub observed_property: Value,
    /// The unit of measurement at the element's position, as served.
    pub unit: Value,
    /// The observation type at the element's position, as served.
    pub observation_type: Value,
}

/// How many `ObservedProperties`, `unitOfMeasurements` and `multiObservationDataTypes` a
/// `MultiDatastream` holds, when they differ in number, so the values of its results cannot be read
/// by position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[display("{observed_properties} ObservedProperties, {units} `unitOfMeasurements` and {observation_types} `multiObservationDataTypes`")]
pub struct ElementCounts {
    /// How many `ObservedProperties` it holds.
    pub observed_properties: usize,
    /// How many units of measurement it holds.
    pub units: usize,
    /// How many observation types it holds.
    pub observation_types: usize,
}

/// Takes the elements of the served `MultiDatastream` `served` out of it, in the order of the values
/// of its results.
///
/// A member that is absent or no array holds no element.
///
/// # Errors
///
/// Returns the [`ElementCounts`] of a `MultiDatastream` whose `ObservedProperties`,
/// `unitOfMeasurements` and `multiObservationDataTypes` differ in number.
pub fn take_elements(served: &mut Map<String, Value>) -> Result<Vec<MultiDatastreamElement>, ElementCounts> {
    let observed_properties = items(served.remove(OBSERVED_PROPERTIES));
    let units = items(served.remove(UNITS));
    let observation_types = items(served.remove(OBSERVATION_TYPES));
    if observed_properties.len() != units.len() || units.len() != observation_types.len() {
        return Err(ElementCounts {
            observed_properties: observed_properties.len(),
            units: units.len(),
            observation_types: observation_types.len(),
        });
    }
    Ok(observed_properties
        .into_iter()
        .zip(units)
        .zip(observation_types)
        .map(|((observed_property, unit), observation_type)| MultiDatastreamElement {
            observed_property,
            unit,
            observation_type,
        })
        .collect())
}

/// The items of `member`, or none when it is absent or no array.
fn items(member: Option<Value>) -> Vec<Value> {
    match member {
        Some(Value::Array(items)) => items,
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) | None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::multi_datastream_elements::{ElementCounts, MultiDatastreamElement, take_elements};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn served(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    #[test]
    fn elements_pair_the_members_by_position_and_leave_the_rest() {
        let mut wind = served(json!({
            "@iot.id": 4,
            "name": "ws-7",
            "ObservedProperties": [{"@iot.id": 11, "name": "windSpeed"}, {"@iot.id": 12, "name": "windDirection"}],
            "unitOfMeasurements": [{"symbol": "m/s"}, {"symbol": "deg"}],
            "multiObservationDataTypes": ["OM_Measurement", "OM_Measurement"],
        }));

        let elements = take_elements(&mut wind).unwrap();

        assert_eq!(
            elements,
            [
                MultiDatastreamElement {
                    observed_property: json!({"@iot.id": 11, "name": "windSpeed"}),
                    unit: json!({"symbol": "m/s"}),
                    observation_type: json!("OM_Measurement"),
                },
                MultiDatastreamElement {
                    observed_property: json!({"@iot.id": 12, "name": "windDirection"}),
                    unit: json!({"symbol": "deg"}),
                    observation_type: json!("OM_Measurement"),
                },
            ]
        );
        assert_eq!(wind, served(json!({"@iot.id": 4, "name": "ws-7"})));
    }

    #[test]
    fn members_that_differ_in_number_are_counted() {
        let mut mismatched = served(json!({
            "ObservedProperties": [{"name": "a"}, {"name": "b"}],
            "unitOfMeasurements": [{}, {}, {}],
            "multiObservationDataTypes": ["x", "y"],
        }));

        let counts = take_elements(&mut mismatched).unwrap_err();

        assert_eq!(
            counts,
            ElementCounts {
                observed_properties: 2,
                units: 3,
                observation_types: 2
            }
        );
        assert_eq!(
            counts.to_string(),
            "2 ObservedProperties, 3 `unitOfMeasurements` and 2 `multiObservationDataTypes`"
        );
    }

    #[test]
    fn a_member_that_is_absent_or_no_array_holds_no_element() {
        let mut without_units = served(json!({"ObservedProperties": [{"name": "a"}], "unitOfMeasurements": {"symbol": "m"}}));
        let mut empty = served(json!({}));

        assert_eq!(
            take_elements(&mut without_units),
            Err(ElementCounts {
                observed_properties: 1,
                units: 0,
                observation_types: 0
            })
        );
        assert_eq!(take_elements(&mut empty), Ok(Vec::new()));
    }
}
