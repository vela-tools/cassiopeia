use crate::{
    error::IngestorError,
    ingestor::Ingestor,
    json::{collection_split, error::JsonIngestError, record_selection::JsonRecordSelection, record_split},
    record_batches::RecordBatches,
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
    path::PathBuf,
    sync::Arc,
};
use tracing::{error, info};

/// Ingestor for JSON data.
///
/// Under [`JsonRecordSelection::LoneArray`] it accepts three top-level shapes: an array of objects
/// (each item a record), a single object (one record), and an envelope (an object with exactly one
/// array-valued member, whose items are the records and whose sibling members are dropped). An object
/// with zero or two-plus array members stays a single record. Under
/// [`JsonRecordSelection::Collections`] it reads the records of the listed nested collections
/// instead, as [`collection_split::split`] describes. `GeoJSON` input is detected and rejected in
/// favour of the `GeoJSON` ingestor.
pub struct JsonIngestor {
    reader: Option<BufReader<File>>,
    /// The file the reader was opened on, so a read failure can name it.
    path: PathBuf,
    batch_size: usize,
    /// Which records the document is split into, shared by every payload of the input.
    selection: Arc<JsonRecordSelection>,
}

impl JsonIngestor {
    /// Creates a JSON ingestor from a profiled payload, splitting its document as `selection` says.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is not a file or the file cannot be opened.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize, selection: Arc<JsonRecordSelection>) -> Result<JsonIngestor, IngestorError> {
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
            selection,
        })
    }
}

impl Ingestor for JsonIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let mut reader = self.reader.take().ok_or(IngestorError::CannotBeStreamedTwice)?;

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

        let mut batches = RecordBatches::new(&sender, self.batch_size);
        match self.selection.as_ref() {
            JsonRecordSelection::LoneArray => record_split::split(content)?.try_for_each(|record| batches.push(record?))?,
            JsonRecordSelection::Collections(labels) => collection_split::split(content, labels, |record| batches.push(record))?,
        }
        batches.finish()
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
        json::{error::JsonIngestError, ingestor::JsonIngestor, record_selection::JsonRecordSelection},
    };
    use cassiopeia_common::{channel::ChannelSender, collection::CollectionName, format::DataFormat, signal::Signal};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use cassiopeia_ir::{
        payload::{CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use mediatype::media_type;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::{
        fs::File,
        io::Write,
        sync::{Arc, mpsc::sync_channel},
        thread,
    };
    use temp_dir::TempDir;

    fn profiled_json(contents: &str) -> (TempDir, ProfiledPayload) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.json");
        File::create(&path).unwrap().write_all(contents.as_bytes()).unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::local(path, Some(DataFormat::Json), EncodingHint::NONE)),
            Profile::new(DataFormat::Json, media_type!(APPLICATION / JSON), 1.0),
        );
        (dir, payload)
    }

    fn try_ingest(payload: ProfiledPayload) -> Result<Vec<Record>, IngestorError> {
        try_ingest_selected(payload, JsonRecordSelection::LoneArray)
    }

    fn try_ingest_selected(payload: ProfiledPayload, selection: JsonRecordSelection) -> Result<Vec<Record>, IngestorError> {
        let ingestor = JsonIngestor::from_payload(payload, 2, Arc::new(selection)).unwrap();
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
        assert_eq!(records[0].collection(), &None);
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

    #[test]
    fn a_collections_selection_reads_the_listed_nested_collection_across_batches() {
        let document = r#"{"data": {"lines": [{"name": "U1", "stops": [{"n": 1}, {"n": 2}, {"n": 3}]}]}}"#;
        let selection = JsonRecordSelection::Collections(vec![CollectionName::from("data.lines.stops")]);
        let records = try_ingest_selected(profiled_json(document).1, selection).unwrap();

        assert_eq!(records.len(), 3);
        assert_eq!(records[2].data().get("n"), Some(&json!(3)));
        assert_eq!(records[2].data().get("lines"), Some(&json!({ "name": "U1" })));
        assert_eq!(records[2].collection(), &Some(CollectionName::from("data.lines.stops")));
    }

    #[test]
    fn a_collections_selection_still_rejects_geojson() {
        let selection = JsonRecordSelection::Collections(vec![CollectionName::from("features")]);
        let result = try_ingest_selected(profiled_json(r#"{"type": "Point", "coordinates": [1.0, 2.0]}"#).1, selection);

        assert!(matches!(result, Err(IngestorError::Json(JsonIngestError::WrongIngestorForGeoJson))));
    }
}
