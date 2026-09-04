use cassiopeia_common::collection::CollectionName;
use getset::Getters;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A raw data record: the entry point of the transformation pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, Getters)]
#[getset(get = "pub")]
pub struct Record {
    /// The source collection this record was read from, when the source packs several.
    ///
    /// A KML folder or an Excel sheet supplies a label here; the single-collection formats (CSV,
    /// JSON, `GeoJSON`) and a folder-less KML placemark leave it `None`. The manifest's mapping
    /// binding routes on this value, so it is kept verbatim rather than as a validated name.
    collection: Option<CollectionName>,
    /// The raw data payload of the record.
    ///
    /// Keys are source-format field or column names, kept verbatim as ingested.
    data: Map<String, Value>,
}

impl Record {
    /// Builds a record from its source collection label and raw source data.
    #[must_use]
    pub const fn new(collection: Option<CollectionName>, data: Map<String, Value>) -> Record {
        Record { collection, data }
    }

    /// Consumes the record and returns its data.
    #[must_use]
    pub fn into_data(self) -> Map<String, Value> {
        self.data
    }
}

#[cfg(test)]
mod tests {
    use crate::record::Record;
    use cassiopeia_common::collection::CollectionName;
    use serde_json::{Map, Value, json};

    #[test]
    fn a_record_without_a_collection_reports_none() {
        let mut data = Map::new();
        data.insert("col".to_string(), json!(1));
        let record = Record::new(None, data.clone());
        assert_eq!(record.collection(), &None);
        assert_eq!(record.data().get("col"), Some(&Value::from(1)));
    }

    #[test]
    fn a_record_carries_its_source_collection_verbatim() {
        let record = Record::new(Some(CollectionName::from("Camera")), Map::new());
        assert_eq!(record.collection(), &Some(CollectionName::from("Camera")));
    }

    #[test]
    fn into_data_yields_the_raw_payload() {
        let mut data = Map::new();
        data.insert("value".to_string(), json!("x"));
        let record = Record::new(None, data.clone());
        assert_eq!(record.into_data(), data);
    }
}
