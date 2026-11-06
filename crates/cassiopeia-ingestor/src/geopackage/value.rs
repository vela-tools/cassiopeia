use base64::{Engine, engine::general_purpose::STANDARD};
use geopackage::ValueRef;
use serde_json::{Number, Value};

/// A column value whose kind this version of Cassiopeia does not know how to read.
///
/// `geopackage::ValueRef` is non-exhaustive, so a later release of the reader may add a kind this
/// conversion has no JSON form for.
#[derive(Debug, thiserror::Error)]
#[error("unsupported GeoPackage value kind")]
pub struct UnsupportedValueKind;

/// Converts one `GeoPackage` column value to the JSON value a record carries under `properties`.
///
/// Returns `Ok(None)` for SQL `NULL`, and for a non-finite `REAL` that JSON cannot represent, so the
/// column is omitted from the record. `DATE` and `DATETIME` values become ISO 8601 strings, a `BLOB`
/// becomes its standard base64 encoding (the form GDAL's `GeoJSON` driver writes binary fields in), and
/// the remaining kinds become their JSON counterparts.
///
/// # Errors
///
/// Returns [`UnsupportedValueKind`] for a value kind this conversion does not know.
pub fn column_value_to_json(value: ValueRef<'_>) -> Result<Option<Value>, UnsupportedValueKind> {
    let json = match value {
        ValueRef::Null => None,
        ValueRef::Boolean(flag) => Some(Value::Bool(flag)),
        ValueRef::Integer(integer) => Some(Value::Number(Number::from(integer))),
        ValueRef::Float(float) => Number::from_f64(float).map(Value::Number),
        ValueRef::Text(text) => Some(Value::String(text.to_string())),
        ValueRef::Blob(bytes) => Some(Value::String(STANDARD.encode(bytes))),
        ValueRef::Date(date) => Some(Value::String(date.to_string())),
        ValueRef::DateTime(datetime) => Some(Value::String(datetime.to_string())),
        // `ValueRef` is `#[non_exhaustive]`: every kind it has today is matched above, so this arm only
        // catches a kind a later release adds.
        _unknown => return Err(UnsupportedValueKind),
    };
    Ok(json)
}

#[cfg(test)]
mod tests {
    use crate::geopackage::value::column_value_to_json;
    use geopackage::ValueRef;
    use geopackage_core::datetime::{Date, DateTime};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn convert(value: ValueRef<'_>) -> Option<Value> {
        column_value_to_json(value).unwrap()
    }

    #[test]
    fn null_is_omitted() {
        assert_eq!(convert(ValueRef::Null), None);
    }

    #[test]
    fn boolean_integer_and_text_map_to_their_json_counterparts() {
        assert_eq!(convert(ValueRef::Boolean(true)), Some(json!(true)));
        assert_eq!(convert(ValueRef::Integer(-42)), Some(json!(-42)));
        assert_eq!(convert(ValueRef::Text("Črešnja")), Some(json!("Črešnja")));
    }

    #[test]
    fn a_finite_float_is_a_number_and_a_non_finite_one_is_omitted() {
        assert_eq!(convert(ValueRef::Float(21.5)), Some(json!(21.5)));
        assert_eq!(convert(ValueRef::Float(f64::NAN)), None);
        assert_eq!(convert(ValueRef::Float(f64::INFINITY)), None);
    }

    #[test]
    fn a_blob_is_its_standard_base64_encoding() {
        assert_eq!(convert(ValueRef::Blob(&[0x00, 0xFF, 0x10])), Some(json!("AP8Q")));
    }

    #[test]
    fn a_date_is_an_iso_8601_date_string() {
        assert_eq!(convert(ValueRef::Date(Date::new(2026, 6, 3).unwrap())), Some(json!("2026-06-03")));
    }

    #[test]
    fn a_datetime_is_an_iso_8601_utc_string_with_millisecond_precision() {
        let datetime = DateTime::parse_strict("2026-06-03T12:30:05.250Z").unwrap();
        assert_eq!(convert(ValueRef::DateTime(datetime)), Some(json!("2026-06-03T12:30:05.250Z")));
    }

    #[test]
    fn a_datetime_with_a_utc_offset_keeps_its_offset() {
        let datetime = DateTime::parse_lenient("2026-06-03T12:30:05+02:00").unwrap();
        assert_eq!(convert(ValueRef::DateTime(datetime)), Some(json!("2026-06-03T12:30:05.000+02:00")));
    }
}
