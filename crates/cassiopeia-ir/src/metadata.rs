use crate::sub_attribute::SubAttributes;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use foldhash::fast::RandomState;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;

/// One entity's attribute-level metadata, keyed by the attribute it qualifies.
///
/// The keys are attribute names a mapping declared, and every attribute of every record probes this
/// map, so it hashes with `foldhash` rather than the standard library's `SipHash`.
pub type EntityMetadata = IndexMap<NameBuf, MetadataStorage, RandomState>;

/// Metadata for one attribute, either shared across all its values or supplied per item.
///
/// Keys are attribute member names such as `observedAt`, `unitCode`, or `datasetId` (ETSI GS CIM 009
/// v1.9.1), and each entry is a [`SubAttribute`] carrying the NGSI-LD kind it was declared as, so a
/// nested Property subclass keeps its kind (clause 4.5.2.2). The reserved qualifiers are stored as
/// plain-Property sub-attributes and lifted by name downstream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, SmartDefault)]
pub enum MetadataStorage {
    /// Shared metadata that applies to the entire attribute.
    /// Example: { "observedAt": "2023-01-01T00:00:00Z", "unitCode": "EUR" }
    #[default]
    Shared(SubAttributes),

    /// Per-item metadata for arrays/lists where each item has its own metadata.
    /// Example: [ { "count": 5 }, { "count": 3 } ]
    PerItem(Vec<SubAttributes>),
}

impl MetadataStorage {
    /// Builds shared metadata that applies to the whole attribute.
    #[must_use]
    pub const fn shared(metadata: SubAttributes) -> MetadataStorage {
        MetadataStorage::Shared(metadata)
    }

    /// Builds per-item metadata, one entry per array element of the attribute.
    #[must_use]
    pub const fn per_item(metadata: Vec<SubAttributes>) -> MetadataStorage {
        MetadataStorage::PerItem(metadata)
    }

    /// Returns the shared metadata map, or `None` when this is per-item storage.
    #[must_use]
    pub const fn as_shared(&self) -> Option<&SubAttributes> {
        match self {
            MetadataStorage::Shared(m) => Some(m),
            MetadataStorage::PerItem(_) => None,
        }
    }

    /// Returns the per-item metadata list, or `None` when this is shared storage.
    #[must_use]
    pub const fn as_per_item(&self) -> Option<&Vec<SubAttributes>> {
        match self {
            MetadataStorage::PerItem(v) => Some(v),
            MetadataStorage::Shared(_) => None,
        }
    }

    /// Metadata for one item at `index`: the shared value for `Shared`, or the entry at `index` for
    /// `PerItem`.
    #[must_use]
    pub fn get_for_index(&self, index: usize) -> Option<&SubAttributes> {
        match self {
            MetadataStorage::Shared(m) => Some(m),
            MetadataStorage::PerItem(v) => v.get(index),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        metadata::MetadataStorage,
        sub_attribute::{SubAttribute, SubAttributes},
    };
    use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};
    use indexmap::IndexMap;
    use serde_json::{Value, json};

    fn entry(key: &str, value: Value) -> SubAttributes {
        let mut map = IndexMap::default();
        map.insert(
            NameBuf::new(key).expect("valid name"),
            SubAttribute::new(NgsiLdAttributeKind::Property, value, IndexMap::default()),
        );
        map
    }

    #[test]
    fn default_is_an_empty_shared_map() {
        let storage = MetadataStorage::default();
        assert_eq!(storage.as_shared(), Some(&IndexMap::default()));
        assert!(storage.as_per_item().is_none());
    }

    #[test]
    fn shared_metadata_is_returned_for_every_index() {
        let storage = MetadataStorage::shared(entry("unitCode", json!("EUR")));
        assert_eq!(storage.get_for_index(0), storage.get_for_index(9));
        assert!(storage.get_for_index(0).is_some());
    }

    #[test]
    fn two_storages_built_from_the_same_entries_compare_equal() {
        // `IndexMap` equality is contents-based and hasher-independent, so a randomly seeded hasher
        // must not make two identically built storages differ.
        let first = MetadataStorage::shared(entry("unitCode", json!("EUR")));
        let second = MetadataStorage::shared(entry("unitCode", json!("EUR")));

        assert_eq!(first, second);
        assert_ne!(first, MetadataStorage::shared(entry("unitCode", json!("KWH"))));
    }

    #[test]
    fn per_item_metadata_is_indexed_and_out_of_range_is_none() {
        let storage = MetadataStorage::per_item(vec![entry("count", json!(5)), entry("count", json!(3))]);
        assert_eq!(storage.get_for_index(1), Some(&entry("count", json!(3))));
        assert!(storage.get_for_index(2).is_none());
        assert!(storage.as_shared().is_none());
    }
}
