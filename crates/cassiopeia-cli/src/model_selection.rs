use crate::{config::CliConfig, error::Result};
use cassiopeia_ngsi_ld::data_model::DataModel;
use cassiopeia_smart_data_models::{
    catalog::{Caching, Catalog},
    store::file_system::FileSystemStore,
};
use std::str::FromStr;

/// The entity data models the stored catalog offers for selection.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the catalog store cannot be opened or its index
/// cannot be read.
pub fn selectable_data_models(config: &CliConfig) -> Result<Vec<DataModel>> {
    let store = FileSystemStore::new(&config.schemas_folder)?;
    let catalog = Catalog::new(store, Caching::Disabled);
    // The index holds only entity models, so this is a defensive typed conversion, not a filter.
    Ok(catalog.list()?.iter().filter_map(|id| DataModel::from_str(&id.to_string()).ok()).collect())
}
