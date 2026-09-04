use cassiopeia_common::collection::CollectionName;
use getset::Getters;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One collection's mapping in a multi-collection source binding.
///
/// Pairs a source collection label (a KML folder name, an Excel sheet name) with the mapping applied
/// to the records read from it. The label is matched verbatim against a record's
/// [`collection`](cassiopeia_ir::record::Record::collection).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[getset(get = "pub")]
pub struct CollectionMapping {
    /// The source collection label this mapping applies to.
    collection: CollectionName,
    /// The mapping document applied to that collection's records.
    mapping: PathBuf,
}

impl CollectionMapping {
    /// Pairs a collection label with the mapping that governs its records.
    #[must_use]
    pub const fn new(collection: CollectionName, mapping: PathBuf) -> CollectionMapping {
        CollectionMapping { collection, mapping }
    }
}

/// How a source's records are routed to mappings.
///
/// A single-collection source (CSV, JSON, `GeoJSON`, or a KML file whose folders are merged) binds
/// to exactly one mapping applied to every record. A multi-collection source (KML with folders,
/// Excel with sheets) binds each collection label to its own mapping, so one un-sliced file yields
/// several NGSI-LD types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappingBinding {
    /// One mapping applied to every record, regardless of its collection label.
    Single {
        /// The sole mapping document.
        mapping: PathBuf,
    },
    /// One mapping per source collection, routed by the record's collection label.
    Collections {
        /// The per-collection mappings, guaranteed non-empty and free of duplicate labels.
        mappings: Vec<CollectionMapping>,
    },
}
