use crate::{error::IngestorError, geojson::error::GeoJsonIngestError, ingestor::Ingestor, record_batches::RecordBatches};
use ::geojson::{Bbox, Feature, FeatureCollection, GeoJson, JsonObject};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use serde_json::{Map, Value};
use std::{fs::File, io::BufReader};

/// The record key under which every feature of a `FeatureCollection` carries the collection's own
/// members, named after the `GeoJSON` object type it holds (RFC 7946 section 3.3).
const FEATURE_COLLECTION: &str = "featureCollection";

/// Ingestor for `GeoJSON` data.
///
/// Supports both `FeatureCollection` and single `Feature` inputs.
pub struct GeoJsonIngestor {
    reader: Option<BufReader<File>>,
    batch_size: usize,
}

impl GeoJsonIngestor {
    /// Creates a `GeoJSON` ingestor from a profiled payload.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is not a file or the file cannot be opened.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize) -> Result<GeoJsonIngestor, IngestorError> {
        let path = match payload.into_payload() {
            CollectedPayload::File(f) => f.into_path(),
            CollectedPayload::Bytes(_) => return Err(GeoJsonIngestError::RequiresFile.into()),
        };

        let file = File::open(&path).map_err(|e| IngestorError::Io { source: e, path: path.clone() })?;

        Ok(GeoJsonIngestor {
            reader: Some(BufReader::new(file)),
            batch_size,
        })
    }

    /// Converts a `GeoJSON` feature into a record with `id`, `properties`, `geometry`, and `bbox`
    /// keys mirroring the source structure.
    ///
    /// When the feature belongs to a `FeatureCollection`, the record also carries that collection's
    /// members under `featureCollection`, so a value the source states once for the whole collection
    /// reaches every feature, the way a KML placemark carries the folder that encloses it.
    fn feature_to_record(feature: Feature, collection: Option<&Map<String, Value>>) -> Result<Record, GeoJsonIngestError> {
        let mut data = Map::new();

        if let Some(id) = &feature.id {
            data.insert("id".to_string(), Self::encode(id)?);
        }

        if let Some(props) = feature.properties {
            data.insert("properties".to_string(), Value::Object(props));
        }

        if let Some(geometry) = &feature.geometry {
            data.insert("geometry".to_string(), Self::encode(geometry)?);
        }

        if let Some(bbox) = &feature.bbox {
            data.insert("bbox".to_string(), Self::encode(bbox)?);
        }

        if let Some(members) = collection {
            // Every record owns its data, so each feature takes its own copy of the members its
            // collection shares among all of them.
            data.insert(FEATURE_COLLECTION.to_string(), Value::Object(members.clone()));
        }

        Ok(Record::new(None, data))
    }

    /// The members a `FeatureCollection` states once for all of its features: its `bbox` when present,
    /// and every foreign member (RFC 7946 section 6.1), such as a timestamp shared by every reading.
    /// Its `type` is always `FeatureCollection`, so it is left out.
    fn collection_members(bbox: Option<Bbox>, foreign_members: Option<JsonObject>) -> Result<Map<String, Value>, GeoJsonIngestError> {
        let mut members = foreign_members.unwrap_or_default();
        if let Some(bbox) = bbox {
            members.insert("bbox".to_string(), Self::encode(&bbox)?);
        }
        Ok(members)
    }

    /// Re-encodes a parsed `GeoJSON` member as the JSON value a record holds.
    fn encode(member: &impl serde::Serialize) -> Result<Value, GeoJsonIngestError> {
        serde_json::to_value(member).map_err(GeoJsonIngestError::Encode)
    }
}

impl Ingestor for GeoJsonIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let reader = self.reader.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let mut batches = RecordBatches::new(&sender, self.batch_size);

        let parsed: Result<GeoJson, _> = serde_json::from_reader(reader);
        match parsed {
            Ok(GeoJson::FeatureCollection(FeatureCollection {
                bbox,
                features,
                foreign_members,
            })) => {
                let members = Self::collection_members(bbox, foreign_members)?;
                for feature in features {
                    batches.push(Self::feature_to_record(feature, Some(&members))?)?;
                }
                batches.finish()
            }
            Ok(GeoJson::Feature(feature)) => {
                batches.push(Self::feature_to_record(feature, None)?)?;
                batches.finish()
            }
            Ok(GeoJson::Geometry(_)) => Err(GeoJsonIngestError::UnsupportedInput.into()),
            Err(e) => Err(GeoJsonIngestError::Parse(::geojson::Error::from(e)).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{geojson::ingestor::GeoJsonIngestor, ingestor::Ingestor};
    use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use cassiopeia_ir::{
        payload::{CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use mediatype::media_type;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::{fs::File, io::Write, sync::mpsc::sync_channel, thread};
    use temp_dir::TempDir;

    fn profiled_geojson(contents: &str) -> (TempDir, ProfiledPayload) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.geojson");
        File::create(&path).unwrap().write_all(contents.as_bytes()).unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::local(path, Some(DataFormat::GeoJson), EncodingHint::NONE)),
            Profile::new(DataFormat::GeoJson, media_type!(APPLICATION / GEO + JSON), 1.0),
        );
        (dir, payload)
    }

    fn ingest_all(payload: ProfiledPayload) -> Vec<Record> {
        let ingestor = GeoJsonIngestor::from_payload(payload, 8).unwrap();
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
    fn a_feature_collection_yields_one_record_per_feature() {
        let contents = r#"{
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "properties": {"name": "A"}, "geometry": {"type": "Point", "coordinates": [1.0, 2.0]}},
                {"type": "Feature", "properties": {"name": "B"}, "geometry": {"type": "Point", "coordinates": [3.0, 4.0]}}
            ]
        }"#;
        let records = ingest_all(profiled_geojson(contents).1);
        assert_eq!(records.len(), 2);
        assert!(records[0].data().contains_key("properties"));
        assert!(records[0].data().contains_key("geometry"));
    }

    #[test]
    fn every_feature_ingests_without_a_collection() {
        let contents = r#"{
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "properties": {"name": "A"}, "geometry": {"type": "Point", "coordinates": [1.0, 2.0]}}
            ]
        }"#;
        let records = ingest_all(profiled_geojson(contents).1);
        assert!(records.iter().all(|record| record.collection().is_none()));
    }

    #[test]
    fn a_single_feature_yields_one_record() {
        let contents = r#"{"type": "Feature", "properties": {"name": "A"}, "geometry": {"type": "Point", "coordinates": [1.0, 2.0]}}"#;
        let records = ingest_all(profiled_geojson(contents).1);
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn every_feature_of_a_collection_carries_the_collection_s_foreign_members() {
        let contents = r#"{
            "type": "FeatureCollection",
            "timestamps": ["2026-10-25T07:50+00:00"],
            "features": [
                {"type": "Feature", "properties": {"station": "11034"}, "geometry": {"type": "Point", "coordinates": [16.37, 48.2]}},
                {"type": "Feature", "properties": {"station": "11035"}, "geometry": {"type": "Point", "coordinates": [16.36, 48.25]}}
            ]
        }"#;
        let records = ingest_all(profiled_geojson(contents).1);
        assert_eq!(records.len(), 2);
        for record in &records {
            assert_eq!(record.data()["featureCollection"], json!({ "timestamps": ["2026-10-25T07:50+00:00"] }));
        }
    }

    #[test]
    fn a_collection_s_bounding_box_reaches_its_features_beside_their_own() {
        let contents = r#"{
            "type": "FeatureCollection",
            "bbox": [0.0, 0.0, 10.0, 10.0],
            "features": [
                {"type": "Feature", "bbox": [1.0, 2.0, 1.0, 2.0], "properties": {}, "geometry": {"type": "Point", "coordinates": [1.0, 2.0]}}
            ]
        }"#;
        let records = ingest_all(profiled_geojson(contents).1);
        assert_eq!(records[0].data()["bbox"], json!([1.0, 2.0, 1.0, 2.0]));
        assert_eq!(records[0].data()["featureCollection"], json!({ "bbox": [0.0, 0.0, 10.0, 10.0] }));
    }

    #[test]
    fn a_collection_without_members_of_its_own_gives_its_features_an_empty_feature_collection() {
        let contents = r#"{
            "type": "FeatureCollection",
            "features": [{"type": "Feature", "properties": {"name": "A"}, "geometry": null}]
        }"#;
        let records = ingest_all(profiled_geojson(contents).1);
        assert_eq!(records[0].data()["featureCollection"], json!({}));
    }

    #[test]
    fn a_single_feature_has_no_feature_collection() {
        let contents = r#"{"type": "Feature", "properties": {"name": "A"}, "geometry": {"type": "Point", "coordinates": [1.0, 2.0]}}"#;
        let records = ingest_all(profiled_geojson(contents).1);
        assert!(!records[0].data().contains_key("featureCollection"));
    }
}
