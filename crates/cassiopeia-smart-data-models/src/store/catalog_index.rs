use crate::schema_id::SchemaId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use url::Url;

/// The index a store keeps beside its schemas.
///
/// Listing the catalog by walking the directory tree costs one `stat` per schema and there are
/// thousands of them, so what is stored is read from this file instead.
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogIndex {
    /// Every schema the store holds, in identifier order.
    #[serde(default)]
    pub models: Vec<SchemaId>,

    /// The `@context` document published for each schema's subject repository.
    #[serde(default)]
    pub contexts: BTreeMap<SchemaId, Url>,
}

impl CatalogIndex {
    /// Replaces the listed models with exactly `models`, sorted and free of duplicates.
    ///
    /// A full-catalog download is authoritative, so recording replaces the list rather than merging
    /// into it: this is what purges support-schema entries left by an earlier download.
    pub fn set_models(&mut self, models: impl IntoIterator<Item = SchemaId>) {
        let mut models: Vec<SchemaId> = models.into_iter().collect();
        models.sort();
        models.dedup();
        self.models = models;
    }
}

#[cfg(test)]
mod tests {
    use crate::{schema_id::SchemaId, store::catalog_index::CatalogIndex};
    use std::str::FromStr;

    fn id(value: &str) -> SchemaId {
        SchemaId::from_str(value).expect("the identifier is well formed")
    }

    #[test]
    fn setting_models_keeps_the_list_sorted_and_free_of_duplicates() {
        let mut index = CatalogIndex::default();

        index.set_models([id("dataModel.OCF/Sensor"), id("common-schema"), id("dataModel.OCF/Sensor")]);

        assert_eq!(index.models, vec![id("common-schema"), id("dataModel.OCF/Sensor")]);
    }

    #[test]
    fn setting_models_replaces_any_earlier_list() {
        let mut index = CatalogIndex::default();

        index.set_models([id("Point"), id("dataModel.OCF/Sensor")]);
        index.set_models([id("dataModel.OCF/Sensor")]);

        assert_eq!(index.models, vec![id("dataModel.OCF/Sensor")]);
    }

    #[test]
    fn an_empty_index_round_trips_through_json() {
        let encoded = serde_json::to_string(&CatalogIndex::default()).unwrap();

        assert_eq!(serde_json::from_str::<CatalogIndex>(&encoded).unwrap(), CatalogIndex::default());
    }

    #[test]
    fn an_index_written_without_contexts_still_reads() {
        let index: CatalogIndex = serde_json::from_str(r#"{"models": ["Point"]}"#).unwrap();

        assert_eq!(index.models, vec![id("Point")]);
        assert!(index.contexts.is_empty());
    }
}
