use crate::shapefile::error::ShapefileIngestError;
use cassiopeia_common::collection::CollectionName;
use cassiopeia_crs::source::SourceCrs;
use cassiopeia_ir::record::Record;
use dbase::{FieldValue, Record as DbaseRecord};
use geo_types::Geometry;
use geojson::{Geometry as GeoJsonGeometry, GeometryValue};
use serde_json::{Map, Number, Value};
use shapefile::Shape;

/// Converts one shapefile feature (its geometry [`Shape`] and its `.dbf` attribute [`DbaseRecord`])
/// into a pipeline [`Record`] with the same shape the `GeoJSON`/KML ingestors produce: a `properties`
/// object plus a `geometry` `GeoJSON` value.
///
/// The geometry is reprojected onto EPSG:4326 via `crs` when a source CRS is known, so the emitted
/// `geometry` is a WGS84 lon/lat `GeoJSON` geometry as NGSI-LD `GeoProperty` values require (RFC 7946
/// clause 4). A [`Shape::NullShape`] contributes no `geometry` key.
///
/// # Errors
///
/// Returns [`ShapefileIngestError`] when the geometry cannot be converted from its shape kind or cannot
/// be reprojected to EPSG:4326.
pub fn shape_and_record_to_record(
    shape: Shape,
    attributes: DbaseRecord,
    crs: Option<&SourceCrs>,
    collection: Option<CollectionName>,
) -> Result<Record, ShapefileIngestError> {
    let mut properties = Map::new();
    for (name, value) in attributes {
        if let Some(json) = field_value_to_json(value) {
            properties.insert(name, json);
        }
    }

    let mut data = Map::new();
    data.insert("properties".to_string(), Value::Object(properties));

    if let Some(geometry) = shape_to_geojson(shape, crs)? {
        data.insert("geometry".to_string(), geometry);
    }

    Ok(Record::new(collection, data))
}

/// Converts a shapefile geometry to a `GeoJSON` value, reprojecting to WGS84 when a source CRS is known.
///
/// Returns `None` for a null geometry, which has no `GeoJSON` equivalent.
fn shape_to_geojson(shape: Shape, crs: Option<&SourceCrs>) -> Result<Option<Value>, ShapefileIngestError> {
    if matches!(shape, Shape::NullShape) {
        return Ok(None);
    }

    let geometry = Geometry::<f64>::try_from(shape).map_err(ShapefileIngestError::Read)?;
    let geometry = match crs {
        Some(crs) => crs.reproject(&geometry)?,
        None => geometry,
    };

    let geojson_geometry = GeoJsonGeometry::new(GeometryValue::from(&geometry));
    Ok(serde_json::to_value(&geojson_geometry).ok())
}

/// Converts a `.dbf` field value to a JSON value, returning `None` for a null field so it is omitted.
///
/// Dates and datetimes serialise to ISO-8601 strings; the numeric kinds to JSON numbers; a non-finite
/// number (which JSON cannot represent) is dropped like a null.
fn field_value_to_json(value: FieldValue) -> Option<Value> {
    match value {
        FieldValue::Character(text) => text.map(Value::String),
        FieldValue::Memo(text) => Some(Value::String(text)),
        FieldValue::Numeric(number) => number.and_then(json_number),
        FieldValue::Float(number) => number.map(f64::from).and_then(json_number),
        FieldValue::Double(number) | FieldValue::Currency(number) => json_number(number),
        FieldValue::Integer(number) => Some(Value::Number(Number::from(number))),
        FieldValue::Logical(flag) => flag.map(Value::Bool),
        FieldValue::Date(date) => date.map(|date| Value::String(format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day()))),
        FieldValue::DateTime(datetime) => {
            let date = datetime.date();
            let time = datetime.time();
            Some(Value::String(format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                date.year(),
                date.month(),
                date.day(),
                time.hours(),
                time.minutes(),
                time.seconds(),
            )))
        }
    }
}

/// Builds a JSON number from an `f64`, returning `None` for a non-finite value JSON cannot represent.
fn json_number(number: f64) -> Option<Value> {
    Number::from_f64(number).map(Value::Number)
}
