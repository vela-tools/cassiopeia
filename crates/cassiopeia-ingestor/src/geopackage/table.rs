use crate::geopackage::{crs_source::read_source_crs, error::GeoPackageIngestError, table_selection::GeoPackageTableSelection};
use cassiopeia_common::collection::CollectionName;
use cassiopeia_crs::source::SourceCrs;
use geopackage::{ContentsDataType, ConversionOptions, GeoPackage, Layer};

/// The data type `gpkg_contents` lists a record table under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableKind {
    /// A `features` table, whose rows carry a geometry.
    Features,
    /// An `attributes` table, whose rows carry none.
    Attributes,
}

impl TableKind {
    /// The kind of record table `data_type` lists, or `None` for a data type that holds no records.
    const fn of(data_type: &ContentsDataType) -> Option<TableKind> {
        match data_type {
            ContentsDataType::Features => Some(TableKind::Features),
            ContentsDataType::Attributes => Some(TableKind::Attributes),
            // Tile and gridded-coverage tables hold raster pyramids rather than records, and an
            // extension's own data type holds nothing this ingestor knows how to read.
            ContentsDataType::Tiles | ContentsDataType::Coverage | ContentsDataType::Other(_) => None,
            // `ContentsDataType` is `#[non_exhaustive]`: every data type it has today is matched above,
            // so this arm only catches one a later release adds.
            _unknown => None,
        }
    }
}

/// One table `gpkg_contents` lists as holding records, not yet opened.
#[derive(Debug)]
pub struct RecordTable {
    /// The table's name, as `gpkg_contents` lists it.
    name: CollectionName,
    /// The data type `gpkg_contents` lists the table under.
    kind: TableKind,
    /// The collection the table's records carry: `None` when `gpkg_contents` lists a single record
    /// table, the table name when it lists several, mirroring how a single-layer shapefile bundle or a
    /// folder-less KML document degrades to no collection.
    collection: Option<CollectionName>,
}

impl RecordTable {
    /// The table's name, as `gpkg_contents` lists it.
    #[must_use]
    pub const fn name(&self) -> &CollectionName {
        &self.name
    }

    /// Opens the table for reading, with the CRS its geometries are reprojected from.
    ///
    /// Values are read leniently: `DATETIME` text in the common ISO 8601 variants real writers produce
    /// is accepted alongside the strict `GeoPackage` 1.4 form, as is a value SQLite stored under a
    /// declared type that does not strictly permit it, such as a `BOOLEAN` column holding `2`.
    ///
    /// # Errors
    ///
    /// Returns [`GeoPackageIngestError::Read`] when the table cannot be opened, such as when no table
    /// of its name exists, [`GeoPackageIngestError::NoGeometryColumn`] when a `features` table has no
    /// `gpkg_geometry_columns` row, or the error of [`read_source_crs`] when its CRS cannot be read.
    pub fn open<'t, 'g>(&'t self, gpkg: &'g GeoPackage) -> Result<OpenTable<'t, 'g>, GeoPackageIngestError> {
        let layer = match self.kind {
            TableKind::Features => gpkg.layer(self.name.as_str())?,
            TableKind::Attributes => gpkg.attributes(self.name.as_str())?,
        };
        // Without its `gpkg_geometry_columns` row (OGC 12-128r19 §2.1.5.1.2, Requirement 22) a feature table's
        // geometry column is not known, so its geometry blobs would be read as a plain binary column.
        if self.kind == TableKind::Features && layer.geometry_column().is_none() {
            return Err(GeoPackageIngestError::NoGeometryColumn { table: self.name.to_string() });
        }
        let crs = read_source_crs(gpkg, &layer)?;

        Ok(OpenTable {
            layer: layer.with_conversion_options(ConversionOptions::lenient()),
            crs,
            collection: self.collection.as_ref(),
        })
    }
}

/// A record table opened for reading.
pub struct OpenTable<'t, 'g> {
    /// The table.
    pub layer: Layer<'g>,
    /// The CRS the table's geometries are reprojected from onto EPSG:4326, or `None` when they need no
    /// reprojection.
    pub crs: Option<SourceCrs>,
    /// The collection the table's records carry.
    pub collection: Option<&'t CollectionName>,
}

/// Lists the tables of `gpkg` that hold records and that `selection` reads, in table-name order.
///
/// A record table is one `gpkg_contents` lists with the `features` or the `attributes` data type;
/// that row may name a view as well as a table (OGC 12-128r19 §1.1.3.1.2, Requirement 14). Each collection label is
/// decided over every record table `gpkg_contents` lists, so a table's label does not change with
/// which tables a run reads or which of them can be read.
///
/// # Errors
///
/// Returns [`GeoPackageIngestError::Read`] when `gpkg_contents` cannot be read, or
/// [`GeoPackageIngestError::NoTables`] when it lists no record table.
pub fn record_tables(gpkg: &GeoPackage, selection: &GeoPackageTableSelection) -> Result<Vec<RecordTable>, GeoPackageIngestError> {
    let listed: Vec<(CollectionName, TableKind)> = gpkg
        .contents()?
        .into_iter()
        .filter_map(|entry| TableKind::of(&entry.data_type).map(|kind| (CollectionName::from(entry.table_name), kind)))
        .collect();
    if listed.is_empty() {
        return Err(GeoPackageIngestError::NoTables);
    }
    let labelled = listed.len() > 1;

    let tables = listed
        .into_iter()
        .filter(|(name, _)| selection.selects(name))
        .map(|(name, kind)| RecordTable {
            // A labelled table's records carry its name, while the table keeps its own to be opened by.
            collection: labelled.then(|| name.clone()),
            name,
            kind,
        })
        .collect();
    Ok(tables)
}

#[cfg(test)]
mod tests {
    use crate::geopackage::{
        error::GeoPackageIngestError,
        table::{RecordTable, TableKind, record_tables},
        table_selection::GeoPackageTableSelection,
    };
    use cassiopeia_common::collection::CollectionName;
    use geopackage::{ColumnSpec, GeoPackage, GeometrySpec, TableSchemaBuilder};
    use geopackage_core::types::{ColumnType, GeometryType};
    use pretty_assertions::assert_eq;
    use std::path::Path;
    use temp_dir::TempDir;

    /// A file listing a `stations` feature table, an `operators` attributes table, and a `basemap`
    /// tile table.
    fn network(dir: &Path) -> GeoPackage {
        let gpkg = GeoPackage::create(dir.join("network.gpkg")).unwrap();
        gpkg.create_layer(
            &TableSchemaBuilder::new("stations")
                .column(ColumnSpec::new("name", ColumnType::Text(None)))
                .geometry(GeometrySpec::new(GeometryType::Point, 4326)),
        )
        .unwrap();
        gpkg.create_attributes_table(&TableSchemaBuilder::new("operators").column(ColumnSpec::new("name", ColumnType::Text(None))))
            .unwrap();
        gpkg.connection()
            .execute(
                "INSERT INTO gpkg_contents (table_name, data_type, identifier) VALUES ('basemap', 'tiles', 'basemap')",
                [],
            )
            .unwrap();
        gpkg
    }

    /// Each table's name, kind, and collection, in order.
    fn listed(tables: &[RecordTable]) -> Vec<(&str, TableKind, Option<&str>)> {
        tables
            .iter()
            .map(|table| (table.name().as_str(), table.kind, table.collection.as_ref().map(CollectionName::as_str)))
            .collect()
    }

    #[test]
    fn features_and_attributes_tables_are_listed_in_name_order_each_labelled_by_its_name() {
        let dir = TempDir::new().unwrap();
        let tables = record_tables(&network(dir.path()), &GeoPackageTableSelection::Every).unwrap();

        assert_eq!(
            listed(&tables),
            [
                ("operators", TableKind::Attributes, Some("operators")),
                ("stations", TableKind::Features, Some("stations")),
            ]
        );
    }

    #[test]
    fn a_lone_record_table_carries_no_collection() {
        let dir = TempDir::new().unwrap();
        let gpkg = GeoPackage::create(dir.path().join("operators.gpkg")).unwrap();
        gpkg.create_attributes_table(&TableSchemaBuilder::new("operators").column(ColumnSpec::new("name", ColumnType::Text(None))))
            .unwrap();

        let tables = record_tables(&gpkg, &GeoPackageTableSelection::Every).unwrap();

        assert_eq!(listed(&tables), [("operators", TableKind::Attributes, None)]);
    }

    #[test]
    fn a_table_the_selection_reads_alone_keeps_the_label_the_whole_catalogue_gives_it() {
        let dir = TempDir::new().unwrap();
        let selection = GeoPackageTableSelection::Listed(vec![CollectionName::from("stations")]);

        let tables = record_tables(&network(dir.path()), &selection).unwrap();

        assert_eq!(listed(&tables), [("stations", TableKind::Features, Some("stations"))]);
    }

    #[test]
    fn a_file_listing_only_tiles_holds_no_record_table() {
        let dir = TempDir::new().unwrap();
        let gpkg = GeoPackage::create(dir.path().join("basemap.gpkg")).unwrap();
        gpkg.connection()
            .execute(
                "INSERT INTO gpkg_contents (table_name, data_type, identifier) VALUES ('basemap', 'tiles', 'basemap')",
                [],
            )
            .unwrap();

        assert!(matches!(
            record_tables(&gpkg, &GeoPackageTableSelection::Every),
            Err(GeoPackageIngestError::NoTables)
        ));
    }

    #[test]
    fn a_feature_table_opens_with_its_crs_and_its_label() {
        let dir = TempDir::new().unwrap();
        let gpkg = network(dir.path());
        let tables = record_tables(&gpkg, &GeoPackageTableSelection::Every).unwrap();

        let stations = tables[1].open(&gpkg).unwrap();

        assert_eq!(stations.layer.table_name(), "stations");
        assert!(stations.layer.geometry_column().is_some());
        assert!(stations.crs.is_none(), "an EPSG:4326 table needs no reprojection");
        assert_eq!(stations.collection, Some(&CollectionName::from("stations")));
    }

    #[test]
    fn a_features_table_without_a_geometry_columns_row_cannot_be_opened() {
        let dir = TempDir::new().unwrap();
        let gpkg = network(dir.path());
        gpkg.connection()
            .execute("UPDATE gpkg_contents SET data_type = 'features' WHERE table_name = 'operators'", [])
            .unwrap();
        let tables = record_tables(&gpkg, &GeoPackageTableSelection::Every).unwrap();

        let error = tables[0].open(&gpkg).err().unwrap();

        assert!(matches!(error, GeoPackageIngestError::NoGeometryColumn { table } if table == "operators"));
    }

    #[test]
    fn a_table_gpkg_contents_lists_but_the_file_lacks_cannot_be_opened() {
        let dir = TempDir::new().unwrap();
        let gpkg = network(dir.path());
        gpkg.connection()
            .execute(
                "INSERT INTO gpkg_contents (table_name, data_type, identifier) VALUES ('ghost', 'attributes', 'ghost')",
                [],
            )
            .unwrap();
        let tables = record_tables(&gpkg, &GeoPackageTableSelection::Every).unwrap();

        assert_eq!(tables[0].name().as_str(), "ghost");
        assert!(matches!(tables[0].open(&gpkg).err().unwrap(), GeoPackageIngestError::Read(_)));
    }
}
