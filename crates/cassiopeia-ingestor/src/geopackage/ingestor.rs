use crate::{error::IngestorError, geopackage::error::GeoPackageIngestError, ingestor::Ingestor};
#[cfg(feature = "geopackage")]
use crate::{
    geopackage::{
        crs_source::read_source_crs,
        record::feature_to_record,
        table::{RecordTable, record_tables},
    },
    record_batches::RecordBatches,
};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
#[cfg(feature = "geopackage")]
use cassiopeia_ir::payload::CollectedPayload;
use cassiopeia_ir::{payload::ProfiledPayload, record::Record};
#[cfg(feature = "geopackage")]
use geopackage::GeoPackage;

/// Ingestor for OGC `GeoPackage`s.
///
/// Every `features` and `attributes` table becomes a stream of records with the same shape as the
/// shapefile, `GeoJSON`, and KML ingestors produce: a `properties` object plus, for a feature, a
/// `geometry` `GeoJSON` value reprojected onto EPSG:4326. A file holding several such tables routes
/// each table to its own collection, keyed by the table name, exactly like shapefile layers or KML
/// folders; a lone table carries no collection.
#[cfg(feature = "geopackage")]
pub struct GeoPackageIngestor {
    geopackage: Option<GeoPackage>,
    batch_size: usize,
}

#[cfg(feature = "geopackage")]
impl GeoPackageIngestor {
    /// Creates a `GeoPackage` ingestor from a profiled payload, opening the file read-only.
    ///
    /// The file is opened leniently, so a `GeoPackage` 1.0 or 1.1 file declaring the legacy `GP10` or
    /// `GP11` `application_id` reads like a current one; anything the reader tolerated while opening is
    /// logged at debug level.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is an in-memory byte buffer rather than a file, or the
    /// file cannot be opened as a `GeoPackage`.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize) -> Result<GeoPackageIngestor, IngestorError> {
        let path = match payload.into_payload() {
            CollectedPayload::File(file) => file.into_path(),
            CollectedPayload::Bytes(_) => return Err(GeoPackageIngestError::RequiresFile.into()),
        };

        let geopackage = GeoPackage::open_read_only_lenient(&path).map_err(GeoPackageIngestError::from)?;
        for warning in geopackage.open_warnings() {
            tracing::debug!(geopackage = %path.display(), ?warning, "GeoPackage opened with a tolerated irregularity");
        }

        Ok(GeoPackageIngestor {
            geopackage: Some(geopackage),
            batch_size,
        })
    }
}

#[cfg(feature = "geopackage")]
impl Ingestor for GeoPackageIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let geopackage = self.geopackage.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let tables = record_tables(&geopackage)?;

        let mut batches = RecordBatches::new(&sender, self.batch_size);
        for table in &tables {
            ingest_table(&geopackage, table, &mut batches)?;
        }
        batches.finish()
    }
}

/// Reads one table's rows into the batches shared by every table of the file.
#[cfg(feature = "geopackage")]
fn ingest_table(geopackage: &GeoPackage, table: &RecordTable<'_>, batches: &mut RecordBatches<'_>) -> Result<(), IngestorError> {
    let crs = read_source_crs(geopackage, &table.layer)?;

    let mut cursor = table.layer.cursor().map_err(GeoPackageIngestError::from)?;
    for feature in cursor.features().map_err(GeoPackageIngestError::from)? {
        let feature = feature.map_err(GeoPackageIngestError::from)?;
        batches.push(feature_to_record(&feature, table, crs.as_ref())?)?;
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
    pub fn from_payload(_payload: ProfiledPayload, _batch_size: usize) -> Result<GeoPackageIngestor, IngestorError> {
        Err(GeoPackageIngestError::NotBuiltIn.into())
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
        geopackage::{error::GeoPackageIngestError, ingestor::GeoPackageIngestor},
    };
    use cassiopeia_common::format::DataFormat;
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use cassiopeia_ir::payload::{CollectedPayload, FilePayload, ProfiledPayload};
    use mediatype::media_type;
    use std::path::PathBuf;

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
        let error = GeoPackageIngestor::from_payload(payload, 2).err().unwrap();
        assert!(matches!(error, IngestorError::GeoPackage(GeoPackageIngestError::NotBuiltIn)));
    }
}

#[cfg(test)]
#[cfg(feature = "geopackage")]
mod tests {
    use crate::{
        error::IngestorError,
        geopackage::{error::GeoPackageIngestError, ingestor::GeoPackageIngestor},
        ingestor::Ingestor,
    };
    use cassiopeia_common::{channel::ChannelSender, collection::CollectionName, format::DataFormat, signal::Signal};
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
        sync::mpsc::sync_channel,
        thread,
    };
    use temp_dir::TempDir;

    /// EPSG:3857 (Web Mercator). At the equator easting is `a * lon` regardless of ellipsoid, so an
    /// easting of 1113194.9079 m at northing 0 reprojects to lon 10, lat 0.
    const WEB_MERCATOR_WKT: &str = r#"PROJCS["WGS 84 / Pseudo-Mercator",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],PROJECTION["Mercator_1SP"],PARAMETER["central_meridian",0],PARAMETER["scale_factor",1],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["metre",1]]"#;

    /// The `srs_id` every `GeoPackage` reserves for an undefined geographic CRS.
    const UNDEFINED_GEOGRAPHIC: i32 = 0;

    fn payload(path: &Path) -> ProfiledPayload {
        ProfiledPayload::new(
            CollectedPayload::File(FilePayload::local(path.to_path_buf(), Some(DataFormat::GeoPackage), EncodingHint::NONE)),
            Profile::new(DataFormat::GeoPackage, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        )
    }

    fn ingest(path: &Path) -> Result<Vec<Record>, IngestorError> {
        let ingestor = GeoPackageIngestor::from_payload(payload(path), 2)?;
        let (tx, rx) = sync_channel(8);
        let handle = thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(tx)));
        let records: Vec<Record> = rx
            .iter()
            .flat_map(|signal| match signal {
                Signal::Data(records) => records,
                Signal::Start | Signal::Stop | Signal::Meta(_) | Signal::Error(_) => panic!("expected a data signal"),
            })
            .collect();
        handle.join().unwrap()?;
        Ok(records)
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

    #[test]
    fn a_single_feature_table_yields_one_record_per_row_with_no_collection() {
        let dir = TempDir::new().unwrap();
        let records = ingest(&stations(dir.path())).unwrap();

        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|record| record.collection().is_none()));
        assert_eq!(properties(&records[0]), json!({ "fid": 1, "name": "Alpha" }).as_object().unwrap());
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

        let error = GeoPackageIngestor::from_payload(payload(&path), 2).err().unwrap();
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
        let error = GeoPackageIngestor::from_payload(payload, 2).err().unwrap();
        assert!(matches!(error, IngestorError::GeoPackage(GeoPackageIngestError::RequiresFile)));
    }

    #[test]
    fn a_consumed_ingestor_cannot_be_streamed_again() {
        let ingestor = GeoPackageIngestor {
            geopackage: None,
            batch_size: 2,
        };
        let (tx, _rx) = sync_channel(2);
        let result = Box::new(ingestor).ingest(ChannelSender::bounded(tx));
        assert!(matches!(result, Err(IngestorError::CannotBeStreamedTwice)));
    }
}
