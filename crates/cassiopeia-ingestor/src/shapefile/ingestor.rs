use crate::{
    error::IngestorError,
    ingestor::Ingestor,
    shapefile::{
        crs_source::read_source_crs,
        error::ShapefileIngestError,
        layer::{Layer, ShapefileSource},
        record::shape_and_record_to_record,
    },
};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use shapefile::{Error as ShapefileError, Reader};
use std::{mem, path::Path};

/// Ingestor for ESRI Shapefiles, in either carrier: a bare local `.shp` (its `.dbf`/`.shx`/`.prj`/
/// `.cpg` companions resolved from the same directory) or a `.zip` bundle of the companion set.
///
/// Each feature becomes a record with the same shape as `GeoJSON`/KML: a `properties` object plus a
/// `geometry` `GeoJSON` value reprojected onto EPSG:4326. A bundle holding several `.shp` layers routes
/// each layer to its own collection, keyed by the layer basename, exactly like KML folders or workbook
/// sheets; a lone layer carries no collection.
#[derive(Debug)]
pub struct ShapefileIngestor {
    source: Option<ShapefileSource>,
    batch_size: usize,
}

impl ShapefileIngestor {
    /// Creates a shapefile ingestor from a profiled payload.
    ///
    /// The carrier (bare `.shp` vs zip bundle) is decided from the collected file's leading bytes, not
    /// its declared format, so a remote `.zip` downloaded under a `.shp` name still ingests correctly.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is an in-memory byte buffer rather than a file, or the
    /// file cannot be opened or classified.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize) -> Result<ShapefileIngestor, IngestorError> {
        let path = match payload.into_payload() {
            CollectedPayload::File(file) => file.into_path(),
            CollectedPayload::Bytes(_) => return Err(ShapefileIngestError::RequiresFile.into()),
        };

        let source = ShapefileSource::from_path(path)?;
        Ok(ShapefileIngestor {
            source: Some(source),
            batch_size,
        })
    }
}

impl Ingestor for ShapefileIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let source = self.source.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let batch_size = self.batch_size;

        // The extraction directory (for a zip bundle) is bound for the whole read: its `Drop` deletes
        // the extracted members, so it must outlive every layer's reader.
        let discovered = source.discover()?;
        let _extraction = discovered.extraction;

        let mut batch_buffer: Vec<Record> = Vec::new();
        for layer in discovered.layers {
            ingest_layer(&layer, batch_size, &sender, &mut batch_buffer)?;
        }

        if !batch_buffer.is_empty() {
            sender.send(Signal::Data(batch_buffer)).map_err(|_| IngestorError::ChannelClosed)?;
        }

        Ok(())
    }
}

/// Reads one layer's features into the shared batch buffer, flushing full batches to the channel.
fn ingest_layer(
    layer: &Layer,
    batch_size: usize,
    sender: &ChannelSender<Signal<Vec<Record>, IngestorError>>,
    batch_buffer: &mut Vec<Record>,
) -> Result<(), IngestorError> {
    let crs = read_source_crs(&layer.shp_path)?;

    let mut reader = match Reader::from_path(&layer.shp_path) {
        Ok(reader) => reader,
        // A `.shp` with no companion `.dbf` cannot carry attribute records; surface it as a typed
        // error rather than the opaque reader failure.
        Err(ShapefileError::MissingDbf) => {
            return Err(ShapefileIngestError::MissingDbf {
                layer: layer_label(&layer.shp_path),
            }
            .into());
        }
        Err(other) => return Err(ShapefileIngestError::Read(other).into()),
    };

    for result in reader.iter_shapes_and_records() {
        let (shape, attributes) = result.map_err(ShapefileIngestError::Read)?;
        let record = shape_and_record_to_record(shape, attributes, crs.as_ref(), layer.collection.clone())?;
        batch_buffer.push(record);

        if batch_buffer.len() >= batch_size {
            let batch = mem::take(batch_buffer);
            sender.send(Signal::Data(batch)).map_err(|_| IngestorError::ChannelClosed)?;
        }
    }

    Ok(())
}

/// A human-readable label for a layer, used in error messages: its `.shp` basename without extension.
fn layer_label(shp_path: &Path) -> String {
    shp_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map_or_else(|| shp_path.display().to_string(), ToString::to_string)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::IngestorError,
        ingestor::Ingestor,
        shapefile::{error::ShapefileIngestError, ingestor::ShapefileIngestor},
    };
    use cassiopeia_common::{channel::ChannelSender, collection::CollectionName, format::DataFormat, signal::Signal};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_ir::{
        payload::{BytesPayload, CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use dbase::{Date, FieldName, FieldValue, Record as DbaseRecord, TableWriterBuilder, yore::code_pages::CP1250};
    use mediatype::media_type;
    use serde_json::{Map, Value};
    use shapefile::{Point as ShpPoint, Writer};
    use std::{
        collections::HashMap,
        fs,
        io::{Cursor, Read, Write},
        path::Path,
        sync::mpsc::sync_channel,
        thread,
    };
    use temp_dir::TempDir;
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

    /// A plain GEOGCS WGS84 definition: coordinates written under it need no reprojection.
    const WGS84_WKT: &str =
        r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;

    /// EPSG:3857 (Web Mercator). At the equator easting is `a * lon` regardless of ellipsoid, so an
    /// easting of 1113194.9079 m at northing 0 reprojects to lon 10, lat 0.
    const WEB_MERCATOR_WKT: &str = r#"PROJCS["WGS 84 / Pseudo-Mercator",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],PROJECTION["Mercator_1SP"],PARAMETER["central_meridian",0],PARAMETER["scale_factor",1],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["metre",1]]"#;

    /// One point feature: its coordinates plus a name and a numeric attribute.
    struct Feature<'a> {
        x: f64,
        y: f64,
        name: &'a str,
        value: f64,
    }

    /// Writes a point layer (`.shp`/`.shx`/`.dbf`) with a fixed attribute schema and, optionally, a
    /// `.prj` sibling. Attribute fields exercise the character, numeric, logical, and date kinds.
    fn write_point_layer(dir: &Path, stem: &str, prj: Option<&str>, features: &[Feature]) {
        let table = TableWriterBuilder::new()
            .add_character_field(FieldName::try_from("name").unwrap(), 50)
            .add_numeric_field(FieldName::try_from("value").unwrap(), 19, 6)
            .add_logical_field(FieldName::try_from("active").unwrap())
            .add_date_field(FieldName::try_from("day").unwrap())
            .build_table_info();
        let mut writer = Writer::from_path_with_info(dir.join(format!("{stem}.shp")), table).unwrap();
        for feature in features {
            let mut record = DbaseRecord::default();
            record.insert("name".to_string(), FieldValue::Character(Some(feature.name.to_string())));
            record.insert("value".to_string(), FieldValue::Numeric(Some(feature.value)));
            record.insert("active".to_string(), FieldValue::Logical(Some(true)));
            // dbase::Date::new takes (day, month, year).
            record.insert("day".to_string(), FieldValue::Date(Some(Date::new(3, 6, 2026).unwrap())));
            writer.write_shape_and_record(&ShpPoint::new(feature.x, feature.y), &record).unwrap();
        }
        drop(writer);
        if let Some(prj) = prj {
            fs::write(dir.join(format!("{stem}.prj")), prj).unwrap();
        }
    }

    /// Builds a profiled payload pointing at a bare `.shp` on disk.
    fn bare_payload(shp_path: &Path) -> ProfiledPayload {
        ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(shp_path.to_path_buf(), Some(DataFormat::Shapefile))),
            Profile::new(DataFormat::Shapefile, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        )
    }

    /// Zips every file in `dir` (flat, by basename) and writes the archive, returning a payload for it.
    fn zip_payload(dir: &Path, archive_path: &Path) -> ProfiledPayload {
        let mut archive = ZipWriter::new(fs::File::create(archive_path).unwrap());
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path == archive_path {
                continue;
            }
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            archive.start_file(name, SimpleFileOptions::default()).unwrap();
            let mut bytes = Vec::new();
            fs::File::open(&path).unwrap().read_to_end(&mut bytes).unwrap();
            archive.write_all(&bytes).unwrap();
        }
        archive.finish().unwrap();
        ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(archive_path.to_path_buf(), Some(DataFormat::Shapefile))),
            Profile::new(DataFormat::Shapefile, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        )
    }

    fn ingest(payload: ProfiledPayload) -> Result<Vec<Record>, IngestorError> {
        let ingestor = ShapefileIngestor::from_payload(payload, 4)?;
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

    /// Looks a property up case-insensitively, since dBase stores field names upper-cased.
    fn property<'a>(properties: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
        properties.iter().find(|(name, _)| name.eq_ignore_ascii_case(key)).map(|(_, value)| value)
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

    fn sample(dir: &Path, stem: &str, prj: Option<&str>) {
        write_point_layer(
            dir,
            stem,
            prj,
            &[
                Feature {
                    x: 14.5,
                    y: 46.05,
                    name: "Alpha",
                    value: 1.5,
                },
                Feature {
                    x: 15.0,
                    y: 46.10,
                    name: "Beta",
                    value: 2.5,
                },
                Feature {
                    x: 15.5,
                    y: 46.15,
                    name: "Gamma",
                    value: 3.5,
                },
            ],
        );
    }

    #[test]
    fn a_bare_point_layer_yields_one_record_per_feature_with_no_collection() {
        let dir = TempDir::new().unwrap();
        sample(dir.path(), "stations", Some(WGS84_WKT));
        let records = ingest(bare_payload(&dir.path().join("stations.shp"))).unwrap();

        assert_eq!(records.len(), 3);
        for record in &records {
            assert_eq!(record.collection(), &None);
            assert!(record.data().contains_key("properties"));
            assert!(record.data().contains_key("geometry"));
        }
    }

    #[test]
    fn dbf_field_kinds_map_to_their_json_types() {
        let dir = TempDir::new().unwrap();
        sample(dir.path(), "stations", Some(WGS84_WKT));
        let records = ingest(bare_payload(&dir.path().join("stations.shp"))).unwrap();

        let properties = properties(&records[0]);
        assert_eq!(property(properties, "name").and_then(Value::as_str), Some("Alpha"));
        assert_eq!(property(properties, "value").and_then(Value::as_f64), Some(1.5));
        assert_eq!(property(properties, "active").and_then(Value::as_bool), Some(true));
        assert_eq!(property(properties, "day").and_then(Value::as_str), Some("2026-06-03"));
    }

    #[test]
    fn a_projected_layer_is_reprojected_to_wgs84_lon_lat() {
        let dir = TempDir::new().unwrap();
        write_point_layer(
            dir.path(),
            "mercator",
            Some(WEB_MERCATOR_WKT),
            &[Feature {
                x: 1_113_194.907_9,
                y: 0.0,
                name: "Equator",
                value: 0.0,
            }],
        );
        let records = ingest(bare_payload(&dir.path().join("mercator.shp"))).unwrap();

        let (lon, lat) = coordinates(&records[0]);
        assert!((lon - 10.0).abs() < 1e-6, "lon was {lon}");
        assert!(lat.abs() < 1e-6, "lat was {lat}");
    }

    #[test]
    fn an_absent_prj_passes_coordinates_through_unchanged() {
        let dir = TempDir::new().unwrap();
        write_point_layer(
            dir.path(),
            "stations",
            None,
            &[Feature {
                x: 14.5,
                y: 46.05,
                name: "Alpha",
                value: 1.5,
            }],
        );
        let records = ingest(bare_payload(&dir.path().join("stations.shp"))).unwrap();

        let (lon, lat) = coordinates(&records[0]);
        assert!((lon - 14.5).abs() < 1e-9, "lon was {lon}");
        assert!((lat - 46.05).abs() < 1e-9, "lat was {lat}");
    }

    #[test]
    fn a_windows_1250_cpg_decodes_accented_attributes_without_mojibake() {
        let dir = TempDir::new().unwrap();
        let stem = "cesta";
        // Write the .dbf bytes in Windows-1250 so a naive UTF-8 read would mangle them.
        let table = TableWriterBuilder::with_encoding(CP1250)
            .add_character_field(FieldName::try_from("name").unwrap(), 50)
            .build_table_info();
        let mut writer = Writer::from_path_with_info(dir.path().join(format!("{stem}.shp")), table).unwrap();
        let mut record = DbaseRecord::default();
        record.insert("name".to_string(), FieldValue::Character(Some("Črešnja".to_string())));
        writer.write_shape_and_record(&ShpPoint::new(14.5, 46.05), &record).unwrap();
        drop(writer);
        // dBase's `.cpg` label table (via `DynEncoding::from_name`) keys Windows-1250 as "CP1250"/"1250",
        // the labels GDAL/QGIS emit; the verbose "windows-1250" spelling is not one it maps.
        fs::write(dir.path().join(format!("{stem}.cpg")), "CP1250").unwrap();

        let records = ingest(bare_payload(&dir.path().join(format!("{stem}.shp")))).unwrap();
        assert_eq!(property(properties(&records[0]), "name").and_then(Value::as_str), Some("Črešnja"));
    }

    #[test]
    fn a_single_layer_zip_matches_the_bare_case() {
        let dir = TempDir::new().unwrap();
        let layer_dir = TempDir::new().unwrap();
        sample(layer_dir.path(), "stations", Some(WGS84_WKT));
        let payload = zip_payload(layer_dir.path(), &dir.path().join("bundle.zip"));
        let records = ingest(payload).unwrap();

        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|record| record.collection().is_none()));
    }

    #[test]
    fn a_multi_layer_zip_tags_records_with_their_layer_collection() {
        let dir = TempDir::new().unwrap();
        let layer_dir = TempDir::new().unwrap();
        write_point_layer(
            layer_dir.path(),
            "roads",
            Some(WGS84_WKT),
            &[
                Feature {
                    x: 14.0,
                    y: 46.0,
                    name: "R1",
                    value: 1.0,
                },
                Feature {
                    x: 14.1,
                    y: 46.1,
                    name: "R2",
                    value: 2.0,
                },
            ],
        );
        write_point_layer(
            layer_dir.path(),
            "poi",
            Some(WGS84_WKT),
            &[
                Feature {
                    x: 15.0,
                    y: 47.0,
                    name: "P1",
                    value: 1.0,
                },
                Feature {
                    x: 15.1,
                    y: 47.1,
                    name: "P2",
                    value: 2.0,
                },
                Feature {
                    x: 15.2,
                    y: 47.2,
                    name: "P3",
                    value: 3.0,
                },
            ],
        );
        let payload = zip_payload(layer_dir.path(), &dir.path().join("bundle.zip"));
        let records = ingest(payload).unwrap();

        let mut per_collection: HashMap<String, usize> = HashMap::new();
        for record in &records {
            let label = record
                .collection()
                .as_ref()
                .map(CollectionName::as_str)
                .expect("a multi-layer bundle tags every record");
            *per_collection.entry(label.to_string()).or_default() += 1;
        }
        assert_eq!(per_collection.get("roads"), Some(&2));
        assert_eq!(per_collection.get("poi"), Some(&3));
    }

    #[test]
    fn a_shp_without_a_companion_dbf_is_a_missing_dbf_error() {
        let dir = TempDir::new().unwrap();
        sample(dir.path(), "stations", Some(WGS84_WKT));
        fs::remove_file(dir.path().join("stations.dbf")).unwrap();

        let error = ingest(bare_payload(&dir.path().join("stations.shp"))).unwrap_err();
        assert!(matches!(error, IngestorError::Shapefile(ShapefileIngestError::MissingDbf { .. })));
    }

    #[test]
    fn a_bytes_payload_is_rejected_because_a_shapefile_needs_files() {
        let payload = ProfiledPayload::new(
            CollectedPayload::Bytes(BytesPayload::new(vec![0x00, 0x00, 0x27, 0x0A], Some(DataFormat::Shapefile))),
            Profile::new(DataFormat::Shapefile, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        );
        let error = ShapefileIngestor::from_payload(payload, 4).unwrap_err();
        assert!(matches!(error, IngestorError::Shapefile(ShapefileIngestError::RequiresFile)));
    }

    #[test]
    fn a_consumed_ingestor_cannot_be_streamed_again() {
        let ingestor = ShapefileIngestor { source: None, batch_size: 4 };
        let (tx, _rx) = sync_channel(4);
        let result = Box::new(ingestor).ingest(ChannelSender::bounded(tx));
        assert!(matches!(result, Err(IngestorError::CannotBeStreamedTwice)));
    }

    #[test]
    fn a_zip_bundle_is_read_by_content_not_extension_and_never_left_on_disk() {
        // The archive is named with a .shp extension yet holds a zip: the carrier must key on the bytes.
        let dir = TempDir::new().unwrap();
        let layer_dir = TempDir::new().unwrap();
        sample(layer_dir.path(), "stations", Some(WGS84_WKT));
        let misnamed = dir.path().join("input.shp");
        let payload = zip_payload(layer_dir.path(), &misnamed);
        // Confirm the fixture really is a zip under a .shp name.
        assert!(ZipArchive::new(Cursor::new(fs::read(&misnamed).unwrap())).is_ok());

        let records = ingest(payload).unwrap();
        assert_eq!(records.len(), 3);
    }
}
