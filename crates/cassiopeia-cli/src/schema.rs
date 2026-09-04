use crate::error::{CliError, IoAction};
use cassiopeia_mapping::mapping::Mapping;
use cassiopeia_ngsi_ld::data_model::DataModel;
use cassiopeia_smart_data_models::{
    catalog::{Caching, Catalog},
    schema_id::SchemaId,
    store::file_system::FileSystemStore,
};
use json5format::{FormatOptions, Json5Format, ParsedDocument};
use serde_json::Value;
use std::{fs, path::Path, str::FromStr};

/// Builds a closure that resolves a data model to its dereferenced JSON Schema.
///
/// The closure is what the explorer and wizard call to load a schema; returning a closure keeps the
/// TUI unaware of the catalog. Failures surface as the CLI's own [`CliError`], which the TUI renders
/// through its `Display` bound.
pub fn create_schema_provider(schemas_folder: &Path) -> impl Fn(&DataModel) -> Result<Value, CliError> {
    let schemas_folder = schemas_folder.to_path_buf();

    move |model: &DataModel| -> Result<Value, CliError> {
        let store = FileSystemStore::new(&schemas_folder)?;
        let catalog = Catalog::new(store, Caching::Enabled);
        let id = SchemaId::from_str(&model.to_string())?;

        catalog
            .get_dereferenced_schema(&id)?
            .ok_or_else(|| CliError::SchemaNotFound { model: model.clone() })
    }
}

/// Builds a closure that writes a finished mapping to the mappings folder as a formatted JSON5 file.
pub fn create_saver(mappings_folder: &Path) -> impl Fn(Mapping) -> Result<(), CliError> {
    let mappings_folder = mappings_folder.to_path_buf();

    move |mapping: Mapping| -> Result<(), CliError> {
        let filename = format!("{}.json5", mapping.data_model().entity_type());
        let path = mappings_folder.join(filename);

        let raw = serde_json::to_string(&mapping).map_err(|source| CliError::SerializeMappingJson { source, path: path.clone() })?;
        let parsed = ParsedDocument::from_str(&raw, None).map_err(|source| CliError::ParseDocumentForFormatting { source, path: path.clone() })?;
        let formatter = Json5Format::with_options(FormatOptions {
            indent_by: 4,
            trailing_commas: true,
            ..Default::default()
        })
        .map_err(CliError::FormatDocument)?;

        let bytes = formatter.to_utf8(&parsed).map_err(CliError::ConvertDocumentToUtf8)?;

        fs::write(&path, bytes).map_err(|source| CliError::FileOperation {
            source,
            path,
            action: IoAction::Write,
        })
    }
}
