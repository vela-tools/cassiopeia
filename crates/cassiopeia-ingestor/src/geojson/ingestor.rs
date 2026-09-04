use crate::{error::IngestorError, geojson::error::GeoJsonIngestError, ingestor::Ingestor};
use ::geojson::{Feature, GeoJson};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use serde_json::{Map, Value};
use std::{fs::File, io::BufReader, mem};

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

    /// Converts a `GeoJSON` feature into a record with `id`, `properties`,
    /// `geometry`, and `bbox` keys mirroring the source structure.
    fn feature_to_record(feature: Feature) -> Result<Record, GeoJsonIngestError> {
        let mut data = Map::new();

        if let Some(id) = &feature.id {
            let encoded = serde_json::to_value(id).map_err(GeoJsonIngestError::Encode)?;
            data.insert("id".to_string(), encoded);
        }

        if let Some(props) = feature.properties {
            data.insert("properties".to_string(), Value::Object(props));
        }

        if let Some(geometry) = feature.geometry
            && let Ok(geo_value) = serde_json::to_value(&geometry)
        {
            data.insert("geometry".to_string(), geo_value);
        }

        if let Some(bbox) = feature.bbox
            && let Ok(bbox_value) = serde_json::to_value(&bbox)
        {
            data.insert("bbox".to_string(), bbox_value);
        }

        Ok(Record::new(None, data))
    }
}

impl Ingestor for GeoJsonIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let reader = self.reader.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let batch_size = self.batch_size;
        let mut batch_buffer: Vec<Record> = Vec::new();

        let parsed: Result<GeoJson, _> = serde_json::from_reader(reader);
        match parsed {
            Ok(GeoJson::FeatureCollection(collection)) => {
                for feature in collection.features {
                    batch_buffer.push(Self::feature_to_record(feature)?);
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
            Ok(GeoJson::Feature(feature)) => {
                let record = Self::feature_to_record(feature)?;
                sender.send(Signal::Data(vec![record])).map_err(|_| IngestorError::ChannelClosed)?;
                Ok(())
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
    use cassiopeia_ir::{
        payload::{CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use mediatype::media_type;
    use std::{fs::File, io::Write, sync::mpsc::sync_channel, thread};
    use temp_dir::TempDir;

    fn profiled_geojson(contents: &str) -> (TempDir, ProfiledPayload) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.geojson");
        File::create(&path).unwrap().write_all(contents.as_bytes()).unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::GeoJson))),
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
}
