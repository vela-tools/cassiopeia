use crate::{
    error::IngestorError,
    ingestor::Ingestor,
    kml::{error::KmlIngestError, extended_data::extract_extended_data, geometry::kml_geometry_to_geojson},
};
use ::kml::{Kml, KmlReader, types::Placemark};
use cassiopeia_common::{channel::ChannelSender, collection::CollectionName, format::DataFormat, signal::Signal};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use convert_case::ccase;
use serde_json::{Map, Value};
use std::{fs::File, io::BufReader, mem, path::PathBuf};

enum KmlSource {
    Kml(BufReader<File>),
    Kmz(PathBuf),
}

/// Ingestor for KML and KMZ (Keyhole Markup Language) data.
///
/// Supports KML and KMZ files with nested `Document` and `Folder` structures. Each
/// `Placemark` becomes a record with the same shape as `GeoJSON`: a `properties`
/// object plus a `geometry` field.
pub struct KmlIngestor {
    reader: Option<KmlSource>,
    batch_size: usize,
}

impl KmlIngestor {
    /// Creates a KML or KMZ ingestor from a profiled payload.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is not a file or the file cannot be opened.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize) -> Result<KmlIngestor, IngestorError> {
        let format = *payload.profile().format();
        let path = match payload.into_payload() {
            CollectedPayload::File(f) => f.into_path(),
            CollectedPayload::Bytes(_) => return Err(KmlIngestError::RequiresFile.into()),
        };

        // KMZ is opened by `KmlReader::from_kmz_path`, which owns the zip machinery; only the
        // plain-KML branch reads the file directly and so needs it opened here.
        let source = match format {
            DataFormat::Kmz => KmlSource::Kmz(path),
            DataFormat::Auto
            | DataFormat::Csv
            | DataFormat::Json
            | DataFormat::GeoJson
            | DataFormat::Kml
            | DataFormat::Grib
            | DataFormat::Shapefile
            | DataFormat::Xml => {
                let file = File::open(&path).map_err(|e| IngestorError::Io { source: e, path })?;
                KmlSource::Kml(BufReader::new(file))
            }
        };

        Ok(KmlIngestor {
            reader: Some(source),
            batch_size,
        })
    }

    /// Converts a KML placemark into a GeoJSON-style record.
    ///
    /// When `namespace` is set (derived from the enclosing folder name), the
    /// record is nested under that key so multiple folders can contribute
    /// different data to the same entity after deep-merging.
    ///
    /// The routing `collection` and the data `namespace` are deliberately distinct views of the same
    /// folder: `collection` keeps the folder name verbatim (`"Camera Area"`) for the manifest to
    /// route on, while `namespace` is its `snake_cased` form (`camera_area`) that wraps the nested data
    /// a `Collections` mapping references as `{{ camera_area.properties.… }}`.
    fn placemark_to_record(folder_name: Option<String>, namespace: Option<&str>, placemark: Placemark, collection: Option<CollectionName>) -> Record {
        let mut inner = Map::new();
        let mut properties = Map::new();

        if let Some(folder) = folder_name {
            properties.insert("folder".to_string(), Value::String(folder));
        }
        if let Some(name) = placemark.name {
            properties.insert("name".to_string(), Value::String(name));
        }
        if let Some(description) = placemark.description {
            properties.insert("description".to_string(), Value::String(description));
        }

        extract_extended_data(&placemark.children, &mut properties);

        if let Some(id) = placemark.attrs.get("id") {
            inner.insert("id".to_string(), Value::String(id.clone()));
        }

        if let Some(geometry) = placemark.geometry
            && let Some(geo_json) = kml_geometry_to_geojson(&geometry)
        {
            inner.insert("geometry".to_string(), geo_json);
        }

        inner.insert("properties".to_string(), Value::Object(properties));

        let data = match namespace {
            Some(ns) => {
                let mut root = Map::new();
                root.insert(ns.to_string(), Value::Object(inner));
                root
            }
            None => inner,
        };

        Record::new(collection, data)
    }

    /// Recursively collects all placemarks, tracking the enclosing folder name
    /// so it can be injected as a property.
    fn collect_placemarks(kml: Kml, folder_name: Option<&str>, placemarks: &mut Vec<(Option<String>, Placemark)>) {
        match kml {
            Kml::KmlDocument(doc) => {
                for element in doc.elements {
                    Self::collect_placemarks(element, folder_name, placemarks);
                }
            }
            Kml::Document { elements, .. } => {
                for element in elements {
                    Self::collect_placemarks(element, folder_name, placemarks);
                }
            }
            Kml::Folder(folder) => {
                let name = folder.name.as_deref().or(folder_name);
                for element in folder.elements {
                    Self::collect_placemarks(element, name, placemarks);
                }
            }
            Kml::Placemark(placemark) => {
                placemarks.push((folder_name.map(str::to_string), placemark));
            }
            // `kml::Kml` is `#[non_exhaustive]`; only container and placemark
            // kinds carry placemarks, everything else is skipped. The trailing
            // wildcard is required to cover hidden future kinds.
            Kml::Scale(_)
            | Kml::Orientation(_)
            | Kml::Point(_)
            | Kml::Location(_)
            | Kml::LineString(_)
            | Kml::LinearRing(_)
            | Kml::Polygon(_)
            | Kml::MultiGeometry(_)
            | Kml::Style(_)
            | Kml::StyleMap(_)
            | Kml::Pair(_)
            | Kml::BalloonStyle(_)
            | Kml::IconStyle(_)
            | Kml::Icon(_)
            | Kml::LabelStyle(_)
            | Kml::LineStyle(_)
            | Kml::PolyStyle(_)
            | Kml::ListStyle(_)
            | Kml::LinkTypeIcon(_)
            | Kml::Link(_)
            | Kml::ResourceMap(_)
            | Kml::Alias(_)
            | Kml::Data(_)
            | Kml::SchemaData(_)
            | Kml::SimpleArrayData(_)
            | Kml::SimpleData(_)
            | Kml::Element(_)
            | _ => {}
        }
    }
}

impl Ingestor for KmlIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let source = self.reader.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let batch_size = self.batch_size;
        let mut batch_buffer: Vec<Record> = Vec::new();

        let kml = match source {
            KmlSource::Kml(reader) => KmlReader::<_, f64>::from_reader(reader).read().map_err(KmlIngestError::Read)?,
            KmlSource::Kmz(path) => KmlReader::<_, f64>::from_kmz_path(path)
                .map_err(KmlIngestError::Read)?
                .read()
                .map_err(KmlIngestError::Read)?,
        };

        let mut placemarks = Vec::new();
        Self::collect_placemarks(kml, None, &mut placemarks);

        for (folder_name, placemark) in placemarks {
            let namespace = folder_name.as_deref().map(|n| ccase!(snake, n));
            // The verbatim folder name is the routing key; a folder-less placemark stays `None` and
            // ingests fine, becoming an error only under a `Collections` mapping binding.
            let collection = folder_name.as_deref().map(CollectionName::from);
            let record = Self::placemark_to_record(folder_name, namespace.as_deref(), placemark, collection);
            batch_buffer.push(record);

            if batch_buffer.len() >= batch_size {
                let batch = mem::take(&mut batch_buffer);
                sender.send(Signal::Data(batch)).map_err(|_| IngestorError::ChannelClosed)?;
            }
        }

        if !batch_buffer.is_empty() {
            sender.send(Signal::Data(batch_buffer)).map_err(|_| IngestorError::ChannelClosed)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{ingestor::Ingestor, kml::ingestor::KmlIngestor};
    use cassiopeia_common::{channel::ChannelSender, collection::CollectionName, format::DataFormat, signal::Signal};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_ir::{
        payload::{CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use mediatype::media_type;
    use serde_json::Value;
    use std::{collections::HashMap, fs::File, io::Write, sync::mpsc::sync_channel, thread};
    use temp_dir::TempDir;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <Placemark>
      <name>Station A</name>
      <Point><coordinates>14.5,46.05,0</coordinates></Point>
    </Placemark>
  </Document>
</kml>"#;

    fn profiled_kml(contents: &str) -> (TempDir, ProfiledPayload) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.kml");
        File::create(&path).unwrap().write_all(contents.as_bytes()).unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Kml))),
            Profile::new(DataFormat::Kml, media_type!(APPLICATION / vnd::GOOGLE_EARTH_KML + XML), 1.0),
        );
        (dir, payload)
    }

    fn profiled_kmz(contents: &str) -> (TempDir, ProfiledPayload) {
        use zip::{ZipWriter, write::SimpleFileOptions};

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.kmz");
        let file = File::create(&path).unwrap();
        let mut archive = ZipWriter::new(file);
        archive.start_file("doc.kml", SimpleFileOptions::default()).unwrap();
        archive.write_all(contents.as_bytes()).unwrap();
        archive.finish().unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Kmz))),
            Profile::new(DataFormat::Kmz, media_type!(APPLICATION / vnd::GOOGLE_EARTH_KMZ), 1.0),
        );
        (dir, payload)
    }

    fn ingest_all(payload: ProfiledPayload) -> Vec<Record> {
        let ingestor = KmlIngestor::from_payload(payload, 8).unwrap();
        let (tx, rx) = sync_channel(4);
        thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(tx)));
        rx.iter()
            .flat_map(|signal| {
                let Signal::Data(records) = signal else {
                    panic!("expected a data signal");
                };
                records
            })
            .collect()
    }

    #[test]
    fn a_placemark_becomes_a_record_with_properties_and_geometry() {
        let records = ingest_all(profiled_kml(SAMPLE).1);
        assert_eq!(records.len(), 1);

        let data = records[0].data();
        let properties = data.get("properties").and_then(Value::as_object).unwrap();
        assert_eq!(properties.get("name"), Some(&Value::String("Station A".to_string())));
        assert!(data.contains_key("geometry"));
    }

    // Mirrors the DenHaag "AI Tech Sensor configuration" file: three sibling folders under one
    // Document, each an implicit schema with its own geometry kind and ExtendedData. Folder names
    // carry a space (routing must keep them verbatim), and counts are 2/2/3.
    const THREE_FOLDERS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <Folder>
      <name>Camera</name>
      <Placemark>
        <name>Cam A</name>
        <ExtendedData><Data name="deviceId"><value>cam-a</value></Data></ExtendedData>
        <Point><coordinates>4.30,52.07,0</coordinates></Point>
      </Placemark>
      <Placemark>
        <name>Cam B</name>
        <ExtendedData><Data name="deviceId"><value>cam-b</value></Data></ExtendedData>
        <Point><coordinates>4.31,52.08,0</coordinates></Point>
      </Placemark>
    </Folder>
    <Folder>
      <name>Camera Area</name>
      <Placemark>
        <name>Area A</name>
        <ExtendedData><Data name="areaId"><value>area-a</value></Data></ExtendedData>
        <LineString><coordinates>4.30,52.07,0 4.31,52.08,0</coordinates></LineString>
      </Placemark>
      <Placemark>
        <name>Area B</name>
        <ExtendedData><Data name="areaId"><value>area-b</value></Data></ExtendedData>
        <LineString><coordinates>4.32,52.09,0 4.33,52.10,0</coordinates></LineString>
      </Placemark>
    </Folder>
    <Folder>
      <name>Flowcount</name>
      <Placemark>
        <name>Flow A</name>
        <ExtendedData><Data name="flowId"><value>flow-a</value></Data></ExtendedData>
        <Polygon><outerBoundaryIs><LinearRing><coordinates>4.30,52.07,0 4.31,52.07,0 4.31,52.08,0 4.30,52.07,0</coordinates></LinearRing></outerBoundaryIs></Polygon>
      </Placemark>
      <Placemark>
        <name>Flow B</name>
        <ExtendedData><Data name="flowId"><value>flow-b</value></Data></ExtendedData>
        <Point><coordinates>4.34,52.11,0</coordinates></Point>
      </Placemark>
      <Placemark>
        <name>Flow C</name>
        <ExtendedData><Data name="flowId"><value>flow-c</value></Data></ExtendedData>
        <Point><coordinates>4.35,52.12,0</coordinates></Point>
      </Placemark>
    </Folder>
  </Document>
</kml>"#;

    const FOLDERLESS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <Placemark>
      <name>Loose</name>
      <Point><coordinates>4.30,52.07,0</coordinates></Point>
    </Placemark>
  </Document>
</kml>"#;

    #[test]
    fn each_placemark_is_tagged_with_its_verbatim_folder_name() {
        let records = ingest_all(profiled_kml(THREE_FOLDERS).1);
        assert_eq!(records.len(), 7);

        let mut per_collection: HashMap<String, usize> = HashMap::new();
        for record in &records {
            let label = record.collection().as_ref().expect("a foldered placemark carries a collection");
            *per_collection.entry(label.as_str().to_string()).or_default() += 1;
        }

        assert_eq!(per_collection.get("Camera"), Some(&2));
        assert_eq!(per_collection.get("Camera Area"), Some(&2));
        assert_eq!(per_collection.get("Flowcount"), Some(&3));
    }

    #[test]
    fn a_foldered_record_nests_its_data_under_the_snake_cased_namespace() {
        let records = ingest_all(profiled_kml(THREE_FOLDERS).1);

        let area = records
            .iter()
            .find(|record| record.collection().as_ref().map(CollectionName::as_str) == Some("Camera Area"))
            .unwrap();
        let nested = area.data().get("camera_area").and_then(Value::as_object).unwrap();
        let properties = nested.get("properties").and_then(Value::as_object).unwrap();
        assert_eq!(properties.get("folder"), Some(&Value::String("Camera Area".to_string())));
        assert_eq!(properties.get("areaId"), Some(&Value::String("area-a".to_string())));
        assert!(nested.contains_key("geometry"));
    }

    #[test]
    fn a_placemark_outside_any_folder_carries_no_collection() {
        let records = ingest_all(profiled_kml(FOLDERLESS).1);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].collection(), &None);
    }

    #[test]
    fn a_kmz_placemark_becomes_a_record_with_properties_and_geometry() {
        let records = ingest_all(profiled_kmz(SAMPLE).1);
        assert_eq!(records.len(), 1);

        let data = records[0].data();
        let properties = data.get("properties").and_then(Value::as_object).unwrap();
        assert_eq!(properties.get("name"), Some(&Value::String("Station A".to_string())));
        assert!(data.contains_key("geometry"));
    }
}
