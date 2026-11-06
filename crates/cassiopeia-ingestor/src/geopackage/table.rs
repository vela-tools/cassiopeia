use crate::geopackage::error::GeoPackageIngestError;
use cassiopeia_common::collection::CollectionName;
use geopackage::{ContentsDataType, ConversionOptions, GeoPackage, Layer};

/// One `GeoPackage` table that holds records, and the collection its records route to.
pub struct RecordTable<'a> {
    /// The table, opened for reading.
    pub layer: Layer<'a>,
    /// The collection this table's records carry: `None` when the file holds a single record table,
    /// the table name when it holds several, mirroring how a single-layer shapefile bundle or a
    /// folder-less KML document degrades to no collection.
    pub collection: Option<CollectionName>,
}

/// Opens every table of `gpkg` that holds records, in table-name order.
///
/// A record table is a table `gpkg_contents` lists with the `features` data type (and that has a
/// `gpkg_geometry_columns` row) or with the `attributes` data type. `tiles` and gridded-coverage tables
/// hold raster pyramids rather than records, so they are not read.
///
/// Values are read leniently: `DATETIME` text in the common ISO 8601 variants real writers produce is
/// accepted alongside the strict `GeoPackage` 1.4 form, as is a value SQLite stored under a declared type
/// that does not strictly permit it, such as a `BOOLEAN` column holding `2`.
///
/// # Errors
///
/// Returns [`GeoPackageIngestError::Read`] when the catalogue or a table cannot be read, or
/// [`GeoPackageIngestError::NoTables`] when the file holds no record table.
pub fn record_tables(gpkg: &GeoPackage) -> Result<Vec<RecordTable<'_>>, GeoPackageIngestError> {
    let mut layers = gpkg.layers()?;
    for entry in gpkg.contents()?.into_iter().filter(|entry| entry.data_type == ContentsDataType::Attributes) {
        layers.push(gpkg.attributes(&entry.table_name)?);
    }
    if layers.is_empty() {
        return Err(GeoPackageIngestError::NoTables);
    }
    layers.sort_by(|left, right| left.table_name().cmp(right.table_name()));

    let single_table = layers.len() == 1;
    let tables = layers
        .into_iter()
        .map(|layer| {
            let collection = if single_table { None } else { Some(CollectionName::from(layer.table_name())) };
            RecordTable {
                layer: layer.with_conversion_options(ConversionOptions::lenient()),
                collection,
            }
        })
        .collect();
    Ok(tables)
}
