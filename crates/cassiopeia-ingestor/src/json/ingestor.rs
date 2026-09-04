use crate::{
    error::IngestorError,
    ingestor::Ingestor,
    json::{error::JsonIngestError, record_split},
};
use ::geojson::GeoJson;
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use serde::Deserialize;
use serde_json::Value;
use simd_json::serde::from_slice;
use std::{
    fs::File,
    io::{BufReader, Read},
    mem,
    path::PathBuf,
};
use tracing::{error, info};

/// Ingestor for JSON data.
///
/// Accepts three top-level shapes under the lone-array convention: an array of objects (each item a
/// record), a single object (one record), and an envelope (an object with exactly one array-valued
/// member, whose items are the records and whose sibling members are dropped). An object with zero or
/// two-plus array members stays a single record. `GeoJSON` input is detected and rejected in favour of
/// the `GeoJSON` ingestor.
pub struct JsonIngestor {
    reader: Option<BufReader<File>>,
    /// The file the reader was opened on, so a read failure can name it.
    path: PathBuf,
    batch_size: usize,
}

impl JsonIngestor {
    /// Creates a JSON ingestor from a profiled payload.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is not a file or the file cannot be opened.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize) -> Result<JsonIngestor, IngestorError> {
        let path = match payload.into_payload() {
            CollectedPayload::File(f) => f.into_path(),
            CollectedPayload::Bytes(_) => return Err(JsonIngestError::RequiresFile.into()),
        };

        let file = File::open(&path).map_err(|source| IngestorError::Io {
            source,
            // The error owns the path after this borrow ends.
            path: path.clone(),
        })?;

        Ok(JsonIngestor {
            reader: Some(BufReader::new(file)),
            path,
            batch_size,
        })
    }
}

impl Ingestor for JsonIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let mut reader = self.reader.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let batch_size = self.batch_size;

        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).map_err(|source| JsonIngestError::Read {
            source,
            path: self.path.clone(),
        })?;

        let content: Value = from_slice(&mut buffer).map_err(JsonIngestError::Parse)?;

        // A GeoJSON document also parses as a JSON object; steer it to the right ingestor.
        if is_geojson(&content) {
            error!("GeoJSON supplied to the JSON ingestor; use the GeoJSON ingestor instead");
            return Err(JsonIngestError::WrongIngestorForGeoJson.into());
        }
        info!("Using JSON ingestor");

        let mut records = record_split::split(content)?;
        let mut batch_buffer = Vec::with_capacity(batch_size.min(records.len()));

        loop {
            let Some(record) = records.next() else {
                break;
            };
            batch_buffer.push(record?);

            if batch_buffer.len() >= batch_size {
                let next_capacity = batch_size.min(records.len());
                let batch = mem::replace(&mut batch_buffer, Vec::with_capacity(next_capacity));
                sender.send(Signal::Data(batch)).map_err(|_| IngestorError::ChannelClosed)?;
            }
        }

        if !batch_buffer.is_empty() {
            sender.send(Signal::Data(batch_buffer)).map_err(|_| IngestorError::ChannelClosed)?;
        }

        Ok(())
    }
}

/// Recognizes valid `GeoJSON` without serializing and reparsing every ordinary JSON document.
fn is_geojson(document: &Value) -> bool {
    let Value::Object(object) = document else {
        return false;
    };
    let Some(Value::String(kind)) = object.get("type") else {
        return false;
    };
    let candidate = matches!(
        kind.as_str(),
        "Feature" | "FeatureCollection" | "GeometryCollection" | "LineString" | "MultiLineString" | "MultiPoint" | "MultiPolygon" | "Point" | "Polygon"
    );

    candidate && GeoJson::deserialize(document).is_ok()
}

#[cfg(test)]
mod tests {
    use crate::{
        error::IngestorError,
        ingestor::Ingestor,
        json::{error::JsonIngestError, ingestor::JsonIngestor},
    };
    use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_ir::{
        payload::{CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use mediatype::media_type;
    use serde_json::json;
    use std::{fs::File, io::Write, sync::mpsc::sync_channel, thread};
    use temp_dir::TempDir;

    fn profiled_json(contents: &str) -> (TempDir, ProfiledPayload) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.json");
        File::create(&path).unwrap().write_all(contents.as_bytes()).unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Json))),
            Profile::new(DataFormat::Json, media_type!(APPLICATION / JSON), 1.0),
        );
        (dir, payload)
    }

    fn try_ingest(payload: ProfiledPayload) -> Result<Vec<Record>, IngestorError> {
        let ingestor = JsonIngestor::from_payload(payload, 8).unwrap();
        let (tx, rx) = sync_channel(4);
        let handle = thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(tx)));
        let mut records = Vec::new();
        for signal in &rx {
            let Signal::Data(batch) = signal else {
                panic!("expected a data signal");
            };
            records.extend(batch);
        }
        handle.join().unwrap()?;
        Ok(records)
    }

    #[test]
    fn an_array_of_objects_becomes_records() {
        let records = try_ingest(profiled_json(r#"[{"a": 1}, {"b": 2}]"#).1).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn every_object_ingests_without_a_collection() {
        let records = try_ingest(profiled_json(r#"[{"a": 1}, {"b": 2}]"#).1).unwrap();
        assert!(records.iter().all(|record| record.collection().is_none()));
    }

    #[test]
    fn a_top_level_object_becomes_one_record() {
        let records = try_ingest(profiled_json(r#"{"a": 1}"#).1).unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].collection().is_none());
    }

    #[test]
    fn an_envelope_ingests_the_data_array() {
        let records = try_ingest(profiled_json(r#"{"meta": 1, "data": [{"a": 1}, {"b": 2}]}"#).1).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn a_non_object_element_is_rejected() {
        let result = try_ingest(profiled_json(r#"[{"a": 1}, 5]"#).1);
        assert!(matches!(result, Err(IngestorError::Json(JsonIngestError::ExpectedObject))));
    }

    #[test]
    fn valid_geojson_is_rejected_in_favour_of_its_ingestor() {
        let result = try_ingest(profiled_json(r#"{"type": "Point", "coordinates": [1.0, 2.0]}"#).1);
        assert!(matches!(result, Err(IngestorError::Json(JsonIngestError::WrongIngestorForGeoJson))));
    }

    #[test]
    fn a_malformed_geojson_candidate_remains_ordinary_json() {
        let records = try_ingest(profiled_json(r#"{"type": "Point", "name": "not a geometry"}"#).1).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("name"), Some(&json!("not a geometry")));
    }
}
