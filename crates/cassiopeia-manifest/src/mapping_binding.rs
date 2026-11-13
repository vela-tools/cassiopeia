use cassiopeia_common::collection::CollectionName;
use getset::Getters;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One collection's mapping in a multi-collection source binding.
///
/// Pairs a source collection label (a KML folder name, an Excel sheet name) with the mapping applied
/// to the records read from it. The label is matched verbatim against the collection an ingested
/// record names.
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

    /// Resolves a relative mapping path against `directory`. An absolute path is kept as it is.
    #[must_use]
    pub fn resolved_against(self, directory: &Path) -> CollectionMapping {
        CollectionMapping {
            mapping: directory.join(self.mapping),
            ..self
        }
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

impl MappingBinding {
    /// Resolves every relative mapping path the binding names against `directory`. An absolute path
    /// is kept as it is.
    #[must_use]
    pub fn resolved_against(self, directory: &Path) -> MappingBinding {
        match self {
            MappingBinding::Single { mapping } => MappingBinding::Single {
                mapping: directory.join(mapping),
            },
            MappingBinding::Collections { mappings } => MappingBinding::Collections {
                mappings: mappings.into_iter().map(|entry| entry.resolved_against(directory)).collect(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::mapping_binding::{CollectionMapping, MappingBinding};
    use cassiopeia_common::collection::CollectionName;
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};

    #[test]
    fn a_single_binding_resolves_its_relative_mapping_against_the_given_directory() {
        let binding = MappingBinding::Single {
            mapping: PathBuf::from("mappings/station.json5"),
        };

        assert_eq!(
            binding.resolved_against(Path::new("runs/city")),
            MappingBinding::Single {
                mapping: PathBuf::from("runs/city/mappings/station.json5")
            }
        );
    }

    #[test]
    fn a_collections_binding_resolves_each_relative_mapping_and_keeps_an_absolute_one() {
        let binding = MappingBinding::Collections {
            mappings: vec![
                CollectionMapping::new(CollectionName::from("A"), PathBuf::from("a.json5")),
                CollectionMapping::new(CollectionName::from("B"), PathBuf::from("/srv/b.json5")),
            ],
        };

        assert_eq!(
            binding.resolved_against(Path::new("runs/city")),
            MappingBinding::Collections {
                mappings: vec![
                    CollectionMapping::new(CollectionName::from("A"), PathBuf::from("runs/city/a.json5")),
                    CollectionMapping::new(CollectionName::from("B"), PathBuf::from("/srv/b.json5")),
                ],
            }
        );
    }
}
