use crate::{
    error::IngestorError,
    ingestor::Ingestor,
    xml::{error::XmlIngestError, record_split::split, value::document_to_value},
};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use std::{
    fs::File,
    io::{BufReader, Read},
    mem,
    path::PathBuf,
};
use tracing::info;

/// Ingestor for generic XML data.
///
/// Reads the whole document, folds it into a value under Cassiopeia's XML convention, and splits it
/// into records on a single repeated child element. It is single-collection like JSON, so every record
/// carries no collection label. External and DTD entities are never expanded, so it is XXE-safe.
pub struct XmlIngestor {
    reader: Option<BufReader<File>>,
    /// The file the reader was opened on, so a read failure can name it.
    path: PathBuf,
    batch_size: usize,
}

impl XmlIngestor {
    /// Creates an XML ingestor from a profiled payload.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is not a file or the file cannot be opened.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize) -> Result<XmlIngestor, IngestorError> {
        let path = match payload.into_payload() {
            CollectedPayload::File(file) => file.into_path(),
            CollectedPayload::Bytes(_) => return Err(XmlIngestError::RequiresFile.into()),
        };

        let file = File::open(&path).map_err(|source| IngestorError::Io { source, path: path.clone() })?;

        Ok(XmlIngestor {
            reader: Some(BufReader::new(file)),
            path,
            batch_size,
        })
    }
}

impl Ingestor for XmlIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let mut reader = self.reader.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let batch_size = self.batch_size;

        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).map_err(|source| XmlIngestError::Read {
            source,
            path: self.path.clone(),
        })?;
        info!("Using XML ingestor");

        let document = document_to_value(&buffer)?;
        let records = split(document);

        let mut batch_buffer: Vec<Record> = Vec::new();
        for record in records {
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
    use crate::{
        error::IngestorError,
        ingestor::Ingestor,
        xml::{error::XmlIngestError, ingestor::XmlIngestor},
    };
    use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_ir::{
        payload::{BytesPayload, CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use mediatype::media_type;
    use serde_json::Value;
    use std::{fs::File, io::Write, path::PathBuf, sync::mpsc::sync_channel, thread};
    use temp_dir::TempDir;

    /// A trimmed ARSO `WebMet` feed with two station-observation blocks; third-party contact/editor
    /// lines are removed, keeping only field shapes. `t_var_unit` carries a degree entity; `tw` is empty.
    const ARSO_TWO_STATIONS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<data>
  <metData>
    <domain_lat>46.0655</domain_lat>
    <domain_title>LJUBLJANA</domain_title>
    <t>24</t>
    <t_var_unit>&#176;C</t_var_unit>
    <rh>52</rh>
    <dd_val>180</dd_val>
    <tw/>
  </metData>
  <metData>
    <domain_lat>46.2389</domain_lat>
    <domain_title>MARIBOR</domain_title>
    <t>22</t>
    <t_var_unit>&#176;C</t_var_unit>
    <rh>60</rh>
    <dd_val>200</dd_val>
    <tw/>
  </metData>
</data>"#;

    /// The same feed with a single station block; `metData` occurs once, so it does not split.
    const ARSO_ONE_STATION: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<data>
  <metData>
    <domain_lat>46.0655</domain_lat>
    <domain_title>LJUBLJANA</domain_title>
    <t>24</t>
    <tw/>
  </metData>
</data>"#;

    fn profiled_xml(contents: &[u8]) -> (TempDir, ProfiledPayload) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.xml");
        File::create(&path).unwrap().write_all(contents).unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Xml))),
            Profile::new(DataFormat::Xml, media_type!(APPLICATION / XML), 0.6),
        );
        (dir, payload)
    }

    fn collect_batches(payload: ProfiledPayload, batch_size: usize) -> Result<Vec<Vec<Record>>, IngestorError> {
        let ingestor = XmlIngestor::from_payload(payload, batch_size)?;
        let (tx, rx) = sync_channel(4);
        let handle = thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(tx)));
        let mut batches = Vec::new();
        for signal in &rx {
            let Signal::Data(batch) = signal else {
                panic!("expected a data signal");
            };
            batches.push(batch);
        }
        handle.join().unwrap()?;
        Ok(batches)
    }

    fn try_ingest(payload: ProfiledPayload) -> Result<Vec<Record>, IngestorError> {
        Ok(collect_batches(payload, 8)?.into_iter().flatten().collect())
    }

    #[test]
    fn a_single_station_feed_ingests_to_one_nested_record() {
        let records = try_ingest(profiled_xml(ARSO_ONE_STATION).1).unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].collection().is_none());
        let met_data = records[0].data().get("metData").unwrap();
        assert_eq!(met_data.get("t"), Some(&Value::String("24".to_owned())));
    }

    #[test]
    fn a_multi_station_feed_splits_into_one_record_per_station() {
        let records = try_ingest(profiled_xml(ARSO_TWO_STATIONS).1).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.collection().is_none()));
        assert_eq!(records[0].data().get("t"), Some(&Value::String("24".to_owned())));
        assert_eq!(records[1].data().get("t"), Some(&Value::String("22".to_owned())));
    }

    #[test]
    fn a_split_record_addresses_fields_directly_and_reads_an_empty_tag_as_null() {
        let records = try_ingest(profiled_xml(ARSO_TWO_STATIONS).1).unwrap();
        assert_eq!(records[0].data().get("domain_lat"), Some(&Value::String("46.0655".to_owned())));
        assert_eq!(records[0].data().get("t_var_unit"), Some(&Value::String("\u{b0}C".to_owned())));
        assert_eq!(records[0].data().get("tw"), Some(&Value::Null));
    }

    #[test]
    fn a_bytes_payload_is_rejected() {
        let payload = ProfiledPayload::new(
            CollectedPayload::Bytes(BytesPayload::new(ARSO_ONE_STATION.to_vec(), Some(DataFormat::Xml))),
            Profile::new(DataFormat::Xml, media_type!(APPLICATION / XML), 0.6),
        );
        let result = XmlIngestor::from_payload(payload, 8);
        assert!(matches!(result, Err(IngestorError::Xml(XmlIngestError::RequiresFile))));
    }

    #[test]
    fn an_already_consumed_ingestor_cannot_be_streamed_twice() {
        let (tx, _rx) = sync_channel(4);
        let ingestor = XmlIngestor {
            reader: None,
            path: PathBuf::from("stations.xml"),
            batch_size: 8,
        };
        let result = Box::new(ingestor).ingest(ChannelSender::bounded(tx));
        assert!(matches!(result, Err(IngestorError::CannotBeStreamedTwice)));
    }

    #[test]
    fn records_are_emitted_in_batches_of_the_configured_size() {
        // Five identical station blocks; only the batch sizes matter here, not the field values.
        let stations = "<metData><t>0</t></metData>".repeat(5);
        let xml = format!(r#"<?xml version="1.0"?><data>{stations}</data>"#);

        let batches = collect_batches(profiled_xml(xml.as_bytes()).1, 2).unwrap();
        let sizes: Vec<usize> = batches.iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![2, 2, 1]);
    }
}
