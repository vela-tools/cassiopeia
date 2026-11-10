use crate::{
    geopackage::{error::GeoPackageIngestError, table::OpenTable, value::column_value_to_json},
    wgs84_geometry::wgs84_geometry_value,
};
use cassiopeia_ir::record::Record;
use geopackage::Feature;
use serde_json::{Map, Number, Value};

/// Converts one `GeoPackage` row into a pipeline [`Record`] with the same shape the shapefile, `GeoJSON`,
/// and KML ingestors produce: a `properties` object plus, for a feature with a non-empty geometry, a
/// `geometry` `GeoJSON` value.
///
/// `properties` holds the table's primary key under its column name (usually `fid`), then every
/// other non-geometry column in schema order; a `NULL` column is omitted. The geometry is reprojected
/// onto EPSG:4326 when the table's CRS needs it, so the emitted `geometry` is a WGS84
/// lon/lat `GeoJSON` geometry as NGSI-LD `GeoProperty` values require (RFC 7946 clause 4). A `NULL`
/// or empty geometry contributes no `geometry` key, like a shapefile null shape.
///
/// # Errors
///
/// Returns [`GeoPackageIngestError`] when the geometry blob cannot be read or represented, a column
/// holds a value of an unknown kind, or the geometry cannot be reprojected to EPSG:4326.
pub fn feature_to_record(feature: &Feature, table: &OpenTable<'_, '_>) -> Result<Record, GeoPackageIngestError> {
    let mut data = Map::new();
    data.insert("properties".to_string(), Value::Object(feature_properties(feature, table)?));
    if let Some(geometry) = feature_geometry(feature, table)? {
        data.insert("geometry".to_string(), geometry);
    }

    // Every record owns its collection label, so each row takes its own copy of its table's.
    Ok(Record::new(table.collection.cloned(), data))
}

/// Builds the `properties` object of one row from its primary key and its value columns.
fn feature_properties(feature: &Feature, table: &OpenTable<'_, '_>) -> Result<Map<String, Value>, GeoPackageIngestError> {
    let mut properties = Map::new();
    // The reader exposes the primary key only as the feature id, so it is restored under its own
    // column name to keep the row's identifying value available to a mapping.
    if let Some(primary_key) = table.layer.primary_key_column() {
        properties.insert(primary_key.to_string(), Value::Number(Number::from(feature.fid())));
    }

    for (column, value) in feature.iter() {
        let json = column_value_to_json(value).map_err(|_| GeoPackageIngestError::UnsupportedValue {
            table: table.layer.table_name().to_string(),
            column: column.to_string(),
        })?;
        if let Some(json) = json {
            properties.insert(column.to_string(), json);
        }
    }
    Ok(properties)
}

/// Reads one row's geometry as a WGS84 `GeoJSON` value, or `None` when it is `NULL` or empty.
fn feature_geometry(feature: &Feature, table: &OpenTable<'_, '_>) -> Result<Option<Value>, GeoPackageIngestError> {
    let Some(geometry) = feature.geometry()?.filter(|geometry| !geometry.is_empty()) else {
        return Ok(None);
    };
    let geometry = geometry.to_geo().ok_or_else(|| GeoPackageIngestError::UnrepresentableGeometry {
        table: table.layer.table_name().to_string(),
        fid: feature.fid(),
    })?;
    Ok(wgs84_geometry_value(geometry, table.crs.as_ref())?)
}
