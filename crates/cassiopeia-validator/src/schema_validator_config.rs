use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use cassiopeia_ngsi_ld::{data_model::DataModelRepository, entity::name::NameBuf};
use std::{collections::HashMap, path::PathBuf};

/// Configuration for a [`SchemaValidator`](crate::schema_validator::SchemaValidator).
pub struct SchemaValidatorConfig {
    /// Directory holding the JSON Schema files, either at its root or nested one level under a
    /// Smart Data Model repository (`<folder>/<repository>/<Type>.json`).
    pub schemas_folder: PathBuf,
    /// The publishing repository for each entity type, taken from the mappings' qualified data
    /// models. A type listed here resolves to `<folder>/<repository>/<Type>.json`, matching the
    /// layout `sdm download` writes; a type absent here resolves to `<folder>/<Type>.json`.
    pub repositories: HashMap<NameBuf, DataModelRepository>,
    /// An explicit schema file for each entity type that requested a custom one, overriding the
    /// convention. A type listed here is validated against its file and, unlike the convention, a
    /// listed file that cannot be loaded is an error rather than a tolerated absence.
    pub custom_schemas: HashMap<NameBuf, PathBuf>,
    /// The NGSI-LD representation entities are serialized in before validation.
    pub representation: NgsiLdRepresentation,
    /// Whether null-valued attributes are skipped during serialization.
    pub skip_null: NgsiLdSkipNull,
}
