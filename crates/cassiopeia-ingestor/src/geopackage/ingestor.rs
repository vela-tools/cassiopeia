use crate::{
    error::IngestorError,
    geopackage::{error::GeoPackageIngestError, table_selection::GeoPackageTableSelection, unreadable_table::UnreadableTable},
    ingestor::Ingestor,
};
#[cfg(feature = "geopackage")]
use crate::{
    geopackage::{
        record::feature_to_record,
        table::{RecordTable, record_tables},
    },
    record_batches::RecordBatches,
};
use cassiopeia_common::{channel::ChannelSender, input::Input, signal::Signal};
#[cfg(feature = "geopackage")]
use cassiopeia_ir::payload::CollectedPayload;
use cassiopeia_ir::{payload::ProfiledPayload, record::Record};
#[cfg(feature = "geopackage")]
use geopackage::GeoPackage;
use parking_lot::Mutex;
use std::sync::Arc;

/// Ingestor for OGC `GeoPackage`s.
///
/// Every `features` and `attributes` table the selection reads becomes a stream of records with the
/// same shape as the shapefile, `GeoJSON`, and KML ingestors produce: a `properties` object plus, for a
/// feature, a `geometry` `GeoJSON` value reprojected onto EPSG:4326. A file listing several such
/// tables routes each table to its own collection, keyed by the table name, exactly like shapefile
/// layers or KML folders; a lone table carries no collection.
///
/// A table that fails before its first row is read, such as a view, which has neither an integer
/// primary key nor a `rowid` to identify its rows by, is skipped and recorded among the
/// [`unreadable tables`](GeoPackageIngestor::unreadable_tables), and the other tables are still read,
/// as a `NetCDF` variable that cannot be read is skipped. The run fails only when no table it reads
/// can be read, or when a row fails.
#[cfg(feature = "geopackage")]
pub struct GeoPackageIngestor {
    geopackage: Option<GeoPackage>,
    /// The input the file was collected from.
    source: Input,
    batch_size: usize,
    selection: Arc<GeoPackageTableSelection>,
    /// The tables skipped so far, shared with whoever reports them once the ingest has run.
    unreadable: Arc<Mutex<Vec<UnreadableTable>>>,
}

#[cfg(feature = "geopackage")]
impl GeoPackageIngestor {
    /// Creates a `GeoPackage` ingestor from a profiled payload, opening the file read-only, that reads
    /// the record tables `selection` picks.
    ///
    /// The file is opened leniently, so a `GeoPackage` 1.0 or 1.1 file declaring the legacy `GP10` or
    /// `GP11` `application_id` reads like a current one; anything the reader tolerated while opening is
    /// logged at debug level.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is an in-memory byte buffer rather than a file, or the
    /// file cannot be opened as a `GeoPackage`.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize, selection: Arc<GeoPackageTableSelection>) -> Result<GeoPackageIngestor, IngestorError> {
        let (path, source) = match payload.into_payload() {
            CollectedPayload::File(file) => file.into_path_and_source(),
            CollectedPayload::Bytes(_) => return Err(GeoPackageIngestError::RequiresFile.into()),
        };

        let geopackage = GeoPackage::open_read_only_lenient(&path).map_err(GeoPackageIngestError::from)?;
        for warning in geopackage.open_warnings() {
            tracing::debug!(geopackage = %path.display(), ?warning, "GeoPackage opened with a tolerated irregularity");
        }

        Ok(GeoPackageIngestor {
            geopackage: Some(geopackage),
            source,
            batch_size,
            selection,
            unreadable: Arc::default(),
        })
    }

    /// The input the file was collected from: the file itself, or the URL a downloaded file was
    /// fetched from, which stays the same from one run to the next where the downloaded copy's path
    /// does not.
    #[must_use]
    pub const fn source(&self) -> &Input {
        &self.source
    }

    /// The tables the ingest skipped because they failed before their first row was read, in table
    /// order, filled in as the file is read: complete once [`Ingestor::ingest`] has returned.
    #[must_use]
    pub fn unreadable_tables(&self) -> Arc<Mutex<Vec<UnreadableTable>>> {
        Arc::clone(&self.unreadable)
    }
}

#[cfg(feature = "geopackage")]
impl Ingestor for GeoPackageIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let geopackage = self.geopackage.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let tables = record_tables(&geopackage, &self.selection)?;

        let mut batches = RecordBatches::new(&sender, self.batch_size);
        let mut read = 0_usize;
        for table in &tables {
            match ingest_table(&geopackage, table, &mut batches) {
                Ok(()) => read += 1,
                Err(TableFailure::Unreadable(reason)) => self.unreadable.lock().push(UnreadableTable {
                    // The record outlives the list of tables it names one of.
                    table: table.name().clone(),
                    reason: Arc::new(reason),
                }),
                Err(TableFailure::Run(error)) => return Err(error),
            }
        }

        // An unreadable table fails before its first row, so a file none of whose tables was read has
        // sent nothing yet.
        if read == 0
            && let Some(first) = self.unreadable.lock().first()
        {
            // The report keeps its own record of the table, so the error takes a copy sharing its reason.
            return Err(GeoPackageIngestError::NoReadableTable(first.clone()).into());
        }
        batches.finish()
    }
}

/// Why reading one record table ended before its last row.
#[cfg(feature = "geopackage")]
enum TableFailure {
    /// The table failed before its first row was read, so it holds no records of the run, and the
    /// other tables are still read.
    Unreadable(GeoPackageIngestError),
    /// A row of the table failed, or its records could not be sent, which stops the run.
    Run(IngestorError),
}

#[cfg(feature = "geopackage")]
impl TableFailure {
    /// A table the reader could not open or select from.
    fn unreadable(error: geopackage::Error) -> TableFailure {
        TableFailure::Unreadable(GeoPackageIngestError::from(error))
    }

    /// A row that failed to read or to convert.
    fn row(error: GeoPackageIngestError) -> TableFailure {
        TableFailure::Run(IngestorError::from(error))
    }
}

/// Reads one table's rows into the batches shared by every table of the file.
#[cfg(feature = "geopackage")]
fn ingest_table(geopackage: &GeoPackage, table: &RecordTable, batches: &mut RecordBatches<'_>) -> Result<(), TableFailure> {
    let table = table.open(geopackage).map_err(TableFailure::Unreadable)?;
    // Preparing the select checks it against the table: the reader identifies each row by the table's
    // integer primary key, or else by its `rowid`, and a view has neither, so it fails here.
    let mut cursor = table.layer.cursor().map_err(TableFailure::unreadable)?;
    for feature in cursor.features().map_err(TableFailure::unreadable)? {
        let feature = feature.map_err(|error| TableFailure::row(error.into()))?;
        batches
            .push(feature_to_record(&feature, &table).map_err(TableFailure::row)?)
            .map_err(TableFailure::Run)?;
    }

    Ok(())
}

/// The `GeoPackage` ingestor of a build without the `geopackage` feature.
///
/// It has no values: [`GeoPackageIngestor::from_payload`] always refuses, so a `GeoPackage` that the
/// profiler still detects is reported as unsupported by this build rather than read as another format.
#[cfg(not(feature = "geopackage"))]
pub enum GeoPackageIngestor {}

#[cfg(not(feature = "geopackage"))]
impl GeoPackageIngestor {
    /// Refuses every payload, because this build links no `GeoPackage` reader.
    ///
    /// # Errors
    ///
    /// Always returns [`GeoPackageIngestError::NotBuiltIn`].
    pub fn from_payload(_payload: ProfiledPayload, _batch_size: usize, _selection: Arc<GeoPackageTableSelection>) -> Result<GeoPackageIngestor, IngestorError> {
        Err(GeoPackageIngestError::NotBuiltIn.into())
    }

    /// The input the file was collected from, which no value of this type has.
    #[must_use]
    pub const fn source(&self) -> &Input {
        match *self {}
    }

    /// The tables the ingest skipped, which no value of this type has.
    #[must_use]
    pub const fn unreadable_tables(&self) -> Arc<Mutex<Vec<UnreadableTable>>> {
        match *self {}
    }
}

#[cfg(not(feature = "geopackage"))]
impl Ingestor for GeoPackageIngestor {
    fn ingest(self: Box<Self>, _sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        match *self {}
    }
}

#[cfg(test)]
#[cfg(not(feature = "geopackage"))]
mod absent_tests {
    use crate::{
        error::IngestorError,
        geopackage::{error::GeoPackageIngestError, ingestor::GeoPackageIngestor, table_selection::GeoPackageTableSelection},
    };
    use cassiopeia_common::format::DataFormat;
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use cassiopeia_ir::payload::{CollectedPayload, FilePayload, ProfiledPayload};
    use mediatype::media_type;
    use std::{path::PathBuf, sync::Arc};

    #[test]
    fn a_build_without_the_geopackage_feature_refuses_a_geopackage_payload() {
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::local(
                PathBuf::from("stations.gpkg"),
                Some(DataFormat::GeoPackage),
                EncodingHint::NONE,
            )),
            Profile::new(DataFormat::GeoPackage, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        );
        let error = GeoPackageIngestor::from_payload(payload, 2, Arc::new(GeoPackageTableSelection::Every))
            .err()
            .unwrap();
        assert!(matches!(error, IngestorError::GeoPackage(GeoPackageIngestError::NotBuiltIn)));
    }
}

#[cfg(test)]
#[cfg(feature = "geopackage")]
mod tests {
    use crate::{
        error::IngestorError,
        geopackage::{
            error::GeoPackageIngestError,
            ingestor::GeoPackageIngestor,
            table_selection::GeoPackageTableSelection,
            unreadable_table::UnreadableTable,
        },
        ingestor::Ingestor,
    };
    use cassiopeia_common::{channel::ChannelSender, collection::CollectionName, format::DataFormat, input::Input, signal::Signal};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use cassiopeia_ir::{
        payload::{BytesPayload, CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use geo_types::{LineString, Point};
    use geopackage::{ColumnSpec, GeoPackage, GeometrySpec, NewFeature, Srs, TableSchemaBuilder, Value as GpkgValue};
    use geopackage_core::{
        datetime::{Date, DateTime},
        types::{ColumnType, GeometryType},
    };
    use mediatype::media_type;
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};
    use std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
        sync::{Arc, mpsc::sync_channel},
        thread,
    };
    use temp_dir::TempDir;

    /// EPSG:3857 (Web Mercator). At the equator easting is `a * lon` regardless of ellipsoid, so an
    /// easting of 1113194.9079 m at northing 0 reprojects to lon 10, lat 0.
    const WEB_MERCATOR_WKT: &str = r#"PROJCS["WGS 84 / Pseudo-Mercator",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],PROJECTION["Mercator_1SP"],PARAMETER["central_meridian",0],PARAMETER["scale_factor",1],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["metre",1]]"#;

    /// EPSG:4258 (ETRS89) as GDAL writes it into `gpkg_spatial_ref_sys`.
    const ETRS89_WKT: &str = r#"GEOGCS["ETRS89",DATUM["European_Terrestrial_Reference_System_1989",SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],AUTHORITY["EPSG","6258"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AXIS["Latitude",NORTH],AXIS["Longitude",EAST],AUTHORITY["EPSG","4258"]]"#;

    /// The `srs_id` every `GeoPackage` reserves for an undefined geographic CRS.
    const UNDEFINED_GEOGRAPHIC: i32 = 0;

    fn payload(path: &Path) -> ProfiledPayload {
        ProfiledPayload::new(
            CollectedPayload::File(FilePayload::local(path.to_path_buf(), Some(DataFormat::GeoPackage), EncodingHint::NONE)),
            Profile::new(DataFormat::GeoPackage, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        )
    }

    fn ingest(path: &Path) -> Result<Vec<Record>, IngestorError> {
        ingest_selected(path, GeoPackageTableSelection::Every)
    }

    fn ingest_selected(path: &Path, selection: GeoPackageTableSelection) -> Result<Vec<Record>, IngestorError> {
        read(path, selection).0
    }

    /// Reads the file's selected tables, with the tables the ingest skipped.
    fn read(path: &Path, selection: GeoPackageTableSelection) -> (Result<Vec<Record>, IngestorError>, Vec<UnreadableTable>) {
        let ingestor = match GeoPackageIngestor::from_payload(payload(path), 2, Arc::new(selection)) {
            Ok(ingestor) => ingestor,
            Err(error) => return (Err(error), Vec::new()),
        };
        let unreadable = ingestor.unreadable_tables();
        let (tx, rx) = sync_channel(8);
        let handle = thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(tx)));
        let records: Vec<Record> = rx
            .iter()
            .flat_map(|signal| match signal {
                Signal::Data(records) => records,
                Signal::Start | Signal::Stop | Signal::Meta(_) | Signal::Error(_) => panic!("expected a data signal"),
            })
            .collect();
        let outcome = handle.join().unwrap().map(|()| records);
        // The ingestor shares the list, and the test keeps a copy of it to compare.
        let skipped = unreadable.lock().clone();
        (outcome, skipped)
    }

    /// The name of each skipped table, in order.
    fn skipped_names(unreadable: &[UnreadableTable]) -> Vec<&str> {
        unreadable.iter().map(|skipped| skipped.table.as_str()).collect()
    }

    fn properties(record: &Record) -> &Map<String, Value> {
        record.data().get("properties").and_then(Value::as_object).unwrap()
    }

    fn coordinates(record: &Record) -> (f64, f64) {
        let geometry = record.data().get("geometry").and_then(Value::as_object).unwrap();
        assert_eq!(geometry.get("type").and_then(Value::as_str), Some("Point"));
        let coords = geometry.get("coordinates").and_then(Value::as_array).unwrap();
        (coords[0].as_f64().unwrap(), coords[1].as_f64().unwrap())
    }

    /// Writes a point layer named `table` with one `name` text column under `srs_id`.
    fn write_point_layer(gpkg: &GeoPackage, table: &str, srs_id: i32, points: &[(f64, f64, &str)]) {
        gpkg.create_layer(
            &TableSchemaBuilder::new(table)
                .column(ColumnSpec::new("name", ColumnType::Text(None)))
                .geometry(GeometrySpec::new(GeometryType::Point, srs_id)),
        )
        .unwrap();
        let features = points
            .iter()
            .map(|(x, y, name)| NewFeature::new(Point::new(*x, *y), vec![GpkgValue::Text((*name).to_string())]));
        gpkg.layer(table).unwrap().write_all(features, 0).unwrap();
    }

    fn stations(dir: &Path) -> PathBuf {
        let path = dir.join("stations.gpkg");
        let gpkg = GeoPackage::create(&path).unwrap();
        write_point_layer(
            &gpkg,
            "stations",
            4326,
            &[(14.5, 46.05, "Alpha"), (15.0, 46.10, "Beta"), (15.5, 46.15, "Gamma")],
        );
        gpkg.close().unwrap();
        path
    }

    /// The `stations` file with a view of the station names registered in `gpkg_contents` as an
    /// attributes table, as VG5000 registers its views.
    fn stations_and_a_view(dir: &Path) -> PathBuf {
        let path = stations(dir);
        let gpkg = GeoPackage::open(&path).unwrap();
        gpkg.connection()
            .execute_batch(
                "CREATE VIEW v_names AS SELECT name FROM stations;
                 INSERT INTO gpkg_contents (table_name, data_type, identifier) VALUES ('v_names', 'attributes', 'v_names');",
            )
            .unwrap();
        gpkg.close().unwrap();
        path
    }

    /// A file whose only record table is a view of a table `gpkg_contents` does not list.
    fn only_a_view(dir: &Path) -> PathBuf {
        let path = dir.join("names.gpkg");
        let gpkg = GeoPackage::create(&path).unwrap();
        gpkg.connection()
            .execute_batch(
                "CREATE TABLE places (name TEXT);
                 INSERT INTO places (name) VALUES ('Alpha');
                 CREATE VIEW v_names AS SELECT name FROM places;
                 INSERT INTO gpkg_contents (table_name, data_type, identifier) VALUES ('v_names', 'attributes', 'v_names');",
            )
            .unwrap();
        gpkg.close().unwrap();
        path
    }

    /// The collection label of every record, in order.
    fn collections(records: &[Record]) -> Vec<Option<&str>> {
        records.iter().map(|record| record.collection().as_ref().map(CollectionName::as_str)).collect()
    }

    #[test]
    fn a_single_feature_table_yields_one_record_per_row_with_no_collection() {
        let dir = TempDir::new().unwrap();
        let records = ingest(&stations(dir.path())).unwrap();

        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|record| record.collection().is_none()));
        assert_eq!(properties(&records[0]), json!({ "fid": 1, "name": "Alpha" }).as_object().unwrap());
    }

    #[test]
    fn the_ingestor_names_the_file_it_reads_as_its_source() {
        let dir = TempDir::new().unwrap();
        let path = stations(dir.path());

        let ingestor = GeoPackageIngestor::from_payload(payload(&path), 2, Arc::new(GeoPackageTableSelection::Every)).unwrap();

        assert_eq!(ingestor.source(), &Input::Local(path));
    }

    #[test]
    fn an_epsg_4326_geometry_is_emitted_with_its_exact_coordinates() {
        let dir = TempDir::new().unwrap();
        let records = ingest(&stations(dir.path())).unwrap();

        assert_eq!(coordinates(&records[0]), (14.5, 46.05));
    }

    #[test]
    fn every_column_type_maps_to_its_json_value_and_a_null_column_is_omitted() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("typed.gpkg");
        let gpkg = GeoPackage::create(&path).unwrap();
        gpkg.create_layer(
            &TableSchemaBuilder::new("readings")
                .column(ColumnSpec::new("label", ColumnType::Text(None)))
                .column(ColumnSpec::new("count", ColumnType::Integer))
                .column(ColumnSpec::new("level", ColumnType::Double))
                .column(ColumnSpec::new("active", ColumnType::Boolean))
                .column(ColumnSpec::new("opened", ColumnType::Date))
                .column(ColumnSpec::new("updated", ColumnType::DateTime))
                .column(ColumnSpec::new("thumbnail", ColumnType::Blob(None)))
                .column(ColumnSpec::new("note", ColumnType::Text(None)))
                .geometry(GeometrySpec::new(GeometryType::Point, 4326)),
        )
        .unwrap();
        gpkg.layer("readings")
            .unwrap()
            .write_all(
                [NewFeature::new(
                    Point::new(14.5, 46.05),
                    vec![
                        GpkgValue::Text("Ljubljana".to_string()),
                        GpkgValue::Integer(3),
                        GpkgValue::Float(21.5),
                        GpkgValue::Boolean(true),
                        GpkgValue::Date(Date::new(2026, 6, 3).unwrap()),
                        GpkgValue::DateTime(DateTime::parse_strict("2026-06-03T12:30:05.250Z").unwrap()),
                        GpkgValue::Blob(vec![0x00, 0xFF, 0x10]),
                        GpkgValue::Null,
                    ],
                )],
                0,
            )
            .unwrap();
        gpkg.close().unwrap();

        let records = ingest(&path).unwrap();
        assert_eq!(
            properties(&records[0]),
            json!({
                "fid": 1,
                "label": "Ljubljana",
                "count": 3,
                "level": 21.5,
                "active": true,
                "opened": "2026-06-03",
                "updated": "2026-06-03T12:30:05.250Z",
                "thumbnail": "AP8Q",
            })
            .as_object()
            .unwrap()
        );
    }

    #[test]
    fn a_projected_table_is_reprojected_to_wgs84_lon_lat() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mercator.gpkg");
        let gpkg = GeoPackage::create(&path).unwrap();
        gpkg.add_srs(&Srs {
            name: "WGS 84 / Pseudo-Mercator".to_string(),
            srs_id: 3857,
            organization: "EPSG".to_string(),
            organization_coordsys_id: 3857,
            definition: WEB_MERCATOR_WKT.to_string(),
            description: None,
            definition_wkt2: None,
            epoch: None,
        })
        .unwrap();
        write_point_layer(&gpkg, "mercator", 3857, &[(1_113_194.907_9, 0.0, "Equator")]);
        gpkg.close().unwrap();

        let (lon, lat) = coordinates(&ingest(&path).unwrap()[0]);
        assert!((lon - 10.0).abs() < 1e-6, "lon was {lon}");
        assert!(lat.abs() < 1e-6, "lat was {lat}");
    }

    #[test]
    fn an_etrs89_geometry_is_emitted_with_the_coordinates_the_source_stored() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("etrs89.gpkg");
        let gpkg = GeoPackage::create(&path).unwrap();
        gpkg.add_srs(&Srs {
            name: "ETRS89".to_string(),
            srs_id: 4258,
            organization: "EPSG".to_string(),
            organization_coordsys_id: 4258,
            definition: ETRS89_WKT.to_string(),
            description: None,
            definition_wkt2: None,
            epoch: None,
        })
        .unwrap();
        write_point_layer(&gpkg, "points", 4258, &[(14.5058, 46.0569, "Ljubljana")]);
        gpkg.close().unwrap();

        assert_eq!(coordinates(&ingest(&path).unwrap()[0]), (14.5058, 46.0569));
    }

    #[test]
    fn an_undefined_crs_passes_coordinates_through_unchanged() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("undefined.gpkg");
        let gpkg = GeoPackage::create(&path).unwrap();
        write_point_layer(&gpkg, "stations", UNDEFINED_GEOGRAPHIC, &[(14.5, 46.05, "Alpha")]);
        gpkg.close().unwrap();

        assert_eq!(coordinates(&ingest(&path).unwrap()[0]), (14.5, 46.05));
    }

    #[test]
    fn a_null_geometry_and_an_empty_geometry_contribute_no_geometry_key() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("tracks.gpkg");
        let gpkg = GeoPackage::create(&path).unwrap();
        gpkg.create_layer(
            &TableSchemaBuilder::new("tracks")
                .column(ColumnSpec::new("name", ColumnType::Text(None)))
                .geometry(GeometrySpec::new(GeometryType::LineString, 4326)),
        )
        .unwrap();
        gpkg.layer("tracks")
            .unwrap()
            .write_all(
                [
                    NewFeature::<LineString<f64>>::attributes(vec![GpkgValue::Text("unsurveyed".to_string())]),
                    NewFeature::new(LineString::<f64>::new(Vec::new()), vec![GpkgValue::Text("empty".to_string())]),
                ],
                0,
            )
            .unwrap();
        gpkg.close().unwrap();

        let records = ingest(&path).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| !record.data().contains_key("geometry")));
    }

    #[test]
    fn several_tables_route_each_to_its_own_collection_and_attribute_rows_carry_no_geometry() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("network.gpkg");
        let gpkg = GeoPackage::create(&path).unwrap();
        write_point_layer(&gpkg, "stations", 4326, &[(14.5, 46.05, "Alpha"), (15.0, 46.10, "Beta")]);
        gpkg.create_attributes_table(&TableSchemaBuilder::new("operators").column(ColumnSpec::new("name", ColumnType::Text(None))))
            .unwrap();
        gpkg.attributes("operators")
            .unwrap()
            .write_all(
                [
                    NewFeature::<Point<f64>>::attributes(vec![GpkgValue::Text("North".to_string())]),
                    NewFeature::<Point<f64>>::attributes(vec![GpkgValue::Text("South".to_string())]),
                    NewFeature::<Point<f64>>::attributes(vec![GpkgValue::Text("East".to_string())]),
                ],
                0,
            )
            .unwrap();
        gpkg.close().unwrap();

        let records = ingest(&path).unwrap();
        let mut per_collection: HashMap<&str, Vec<&Record>> = HashMap::new();
        for record in &records {
            let label = record
                .collection()
                .as_ref()
                .map(CollectionName::as_str)
                .expect("a multi-table GeoPackage tags every record");
            per_collection.entry(label).or_default().push(record);
        }

        assert_eq!(per_collection.get("stations").map(Vec::len), Some(2));
        assert_eq!(per_collection.get("operators").map(Vec::len), Some(3));
        assert!(per_collection["stations"].iter().all(|record| record.data().contains_key("geometry")));
        assert!(per_collection["operators"].iter().all(|record| !record.data().contains_key("geometry")));
    }

    #[test]
    fn a_view_is_skipped_and_every_other_table_is_read() {
        let dir = TempDir::new().unwrap();
        let (records, unreadable) = read(&stations_and_a_view(dir.path()), GeoPackageTableSelection::Every);
        let records = records.unwrap();

        assert_eq!(skipped_names(&unreadable), ["v_names"]);
        assert!(unreadable[0].reason.to_string().contains("no such column: rowid"), "{}", unreadable[0].reason);
        assert_eq!(collections(&records), [Some("stations"); 3]);
        assert_eq!(properties(&records[2]), json!({ "fid": 3, "name": "Gamma" }).as_object().unwrap());
    }

    #[test]
    fn a_table_gpkg_contents_lists_but_the_file_lacks_is_skipped_and_every_other_table_is_read() {
        let dir = TempDir::new().unwrap();
        let path = stations(dir.path());
        let gpkg = GeoPackage::open(&path).unwrap();
        gpkg.connection()
            .execute(
                "INSERT INTO gpkg_contents (table_name, data_type, identifier) VALUES ('ghost', 'attributes', 'ghost')",
                [],
            )
            .unwrap();
        gpkg.close().unwrap();

        let (records, unreadable) = read(&path, GeoPackageTableSelection::Every);

        assert_eq!(skipped_names(&unreadable), ["ghost"]);
        assert!(matches!(*unreadable[0].reason, GeoPackageIngestError::Read(_)));
        assert_eq!(collections(&records.unwrap()), [Some("stations"); 3]);
    }

    #[test]
    fn a_geopackage_none_of_whose_tables_can_be_read_fails_naming_the_first_and_why() {
        let dir = TempDir::new().unwrap();
        let (outcome, unreadable) = read(&only_a_view(dir.path()), GeoPackageTableSelection::Every);
        let error = outcome.unwrap_err();

        assert_eq!(skipped_names(&unreadable), ["v_names"]);
        assert_eq!(error.to_string(), "no record table of the GeoPackage can be read");
        assert!(
            matches!(
                &error,
                IngestorError::GeoPackage(GeoPackageIngestError::NoReadableTable(first))
                    if first.table == CollectionName::from("v_names")
                        && matches!(*first.reason, GeoPackageIngestError::Read(_))
                        && first.reason.to_string().contains("no such column: rowid")
            ),
            "{error:?}"
        );
    }

    #[test]
    fn a_listed_selection_reads_only_the_tables_it_names_and_each_keeps_its_label() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("network.gpkg");
        let gpkg = GeoPackage::create(&path).unwrap();
        write_point_layer(&gpkg, "stations", 4326, &[(14.5, 46.05, "Alpha"), (15.0, 46.10, "Beta")]);
        write_point_layer(&gpkg, "depots", 4326, &[(14.6, 46.06, "North")]);
        gpkg.close().unwrap();

        let records = ingest_selected(&path, GeoPackageTableSelection::Listed(vec![CollectionName::from("depots")])).unwrap();

        assert_eq!(collections(&records), [Some("depots")]);
        assert_eq!(properties(&records[0]), json!({ "fid": 1, "name": "North" }).as_object().unwrap());
    }

    #[test]
    fn a_table_the_selection_does_not_name_is_left_unread_so_it_cannot_fail_the_run() {
        let dir = TempDir::new().unwrap();
        let path = only_a_view(dir.path());

        let (records, unreadable) = read(&path, GeoPackageTableSelection::Listed(vec![CollectionName::from("places")]));

        assert!(records.unwrap().is_empty());
        assert!(unreadable.is_empty());
        assert!(matches!(
            ingest(&path),
            Err(IngestorError::GeoPackage(GeoPackageIngestError::NoReadableTable { .. }))
        ));
    }

    #[test]
    fn a_geopackage_without_a_record_table_is_a_no_tables_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("empty.gpkg");
        GeoPackage::create(&path).unwrap().close().unwrap();

        let error = ingest(&path).unwrap_err();
        assert!(matches!(error, IngestorError::GeoPackage(GeoPackageIngestError::NoTables)));
    }

    #[test]
    fn a_file_that_is_not_a_geopackage_is_a_read_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("not.gpkg");
        fs::write(&path, "id,name\n1,alpha\n").unwrap();

        let error = GeoPackageIngestor::from_payload(payload(&path), 2, Arc::new(GeoPackageTableSelection::Every))
            .err()
            .unwrap();
        assert!(matches!(error, IngestorError::GeoPackage(GeoPackageIngestError::Read(_))));
    }

    #[test]
    fn a_bytes_payload_is_rejected_because_sqlite_opens_a_file() {
        let payload = ProfiledPayload::new(
            CollectedPayload::Bytes(BytesPayload::new(
                b"SQLite format 3\0".to_vec(),
                Some(DataFormat::GeoPackage),
                EncodingHint::NONE,
            )),
            Profile::new(DataFormat::GeoPackage, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        );
        let error = GeoPackageIngestor::from_payload(payload, 2, Arc::new(GeoPackageTableSelection::Every))
            .err()
            .unwrap();
        assert!(matches!(error, IngestorError::GeoPackage(GeoPackageIngestError::RequiresFile)));
    }

    #[test]
    fn a_consumed_ingestor_cannot_be_streamed_again() {
        let ingestor = GeoPackageIngestor {
            geopackage: None,
            source: Input::Local(PathBuf::from("stations.gpkg")),
            batch_size: 2,
            selection: Arc::new(GeoPackageTableSelection::Every),
            unreadable: Arc::default(),
        };
        let (tx, _rx) = sync_channel(2);
        let result = Box::new(ingestor).ingest(ChannelSender::bounded(tx));
        assert!(matches!(result, Err(IngestorError::CannotBeStreamedTwice)));
    }
}
