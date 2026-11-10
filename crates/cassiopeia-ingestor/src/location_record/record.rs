use crate::location_record::{facets::LocationFacets, parameter_name::ParameterName};
use cassiopeia_ir::record::Record;
use serde_json::{Map, Number, Value};

/// The record key a cell's latitude is emitted under.
pub const LATITUDE: &str = "latitude";

/// The record key a cell's longitude is emitted under.
pub const LONGITUDE: &str = "longitude";

/// Builds the wide location record of one cell or observation: its coordinates, the facets it shares
/// with the rest of its slice, and one column per measured value.
///
/// Coordinates are emitted as the separate `latitude` and `longitude` scalars a mapping composes into
/// a `Point` `GeoProperty` (RFC 7946 order), rather than a pre-built coordinate pair. A masked column
/// (`NaN`) is omitted from the record, and a cell where every column is masked yields no record at
/// all. The record carries no collection: a gridded or station file is one flat table of locations.
pub fn location_record<'a>(latitude: f64, longitude: f64, facets: &LocationFacets, columns: impl Iterator<Item = (&'a ParameterName, f64)>) -> Option<Record> {
    let mut columns = columns.filter(|(_, value)| !value.is_nan()).peekable();
    columns.peek()?;

    let mut data = Map::new();
    insert_number(&mut data, LATITUDE, latitude);
    insert_number(&mut data, LONGITUDE, longitude);
    // The facets are shared by every cell of the slice, so each record takes its own copy.
    data.extend(facets.into_iter().map(|(key, value)| (key.clone(), value.clone())));
    for (name, value) in columns {
        insert_number(&mut data, name.as_str(), value);
    }
    Some(Record::new(None, data))
}

/// Inserts a finite float as a JSON number, omitting the field when the value cannot be represented.
fn insert_number(data: &mut Map<String, Value>, key: &str, value: f64) {
    if let Some(number) = Number::from_f64(value) {
        data.insert(key.to_string(), Value::Number(number));
    }
}

#[cfg(test)]
mod tests {
    use crate::location_record::{
        facets::LocationFacets,
        level::{Level, LevelType},
        parameter_name::ParameterName,
        record::location_record,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn names() -> (ParameterName, ParameterName) {
        (ParameterName::from("t"), ParameterName::from("r"))
    }

    #[test]
    fn a_record_carries_its_coordinates_then_its_facets_then_its_columns() {
        let (t, r) = names();
        let facets = LocationFacets::default().with_time(&ParameterName::from("time"), "2026-01-01T00:00:00+00:00".to_string());
        let record = location_record(46.0, 14.5, &facets, [(&t, 280.5), (&r, 0.25)].into_iter()).unwrap();

        let keys: Vec<&str> = record.data().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["latitude", "longitude", "time", "t", "r"]);
        assert_eq!(record.data().get("latitude"), Some(&json!(46.0)));
        assert_eq!(record.data().get("longitude"), Some(&json!(14.5)));
        assert_eq!(record.data().get("t"), Some(&json!(280.5)));
        assert_eq!(record.collection(), &None);
    }

    #[test]
    fn a_masked_column_is_omitted() {
        let (t, r) = names();
        let record = location_record(46.0, 14.5, &LocationFacets::default(), [(&t, f64::NAN), (&r, 0.25)].into_iter()).unwrap();
        assert!(!record.data().contains_key("t"));
        assert_eq!(record.data().get("r"), Some(&json!(0.25)));
    }

    #[test]
    fn a_cell_masked_in_every_column_yields_no_record() {
        let (t, r) = names();
        assert!(location_record(46.0, 14.5, &LocationFacets::default(), [(&t, f64::NAN), (&r, f64::NAN)].into_iter()).is_none());
    }

    #[test]
    fn a_whole_level_is_emitted_as_an_integer() {
        let (t, _) = names();
        let facets = LocationFacets::default().with_level(&Level::new(LevelType::Isobaric, Some(850.0)));
        let record = location_record(46.0, 14.5, &facets, [(&t, 280.5)].into_iter()).unwrap();
        assert_eq!(record.data().get("level"), Some(&json!(850)));
        assert_eq!(record.data().get("level").and_then(Value::as_i64), Some(850));
    }
}
