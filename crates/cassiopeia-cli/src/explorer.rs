use crate::{config::CliConfig, error::Result, model_selection::selectable_data_models, schema::create_schema_provider, theme_choice::ThemeChoice};
use cassiopeia_tui::{config::TuiConfig, screen::explorer::builder::ExplorerBuilder};

/// Launches the read-only schema explorer over the stored Smart Data Model catalog.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the catalog store cannot be opened or the
/// explorer fails to run.
pub fn launch_explorer(config: &CliConfig, theme: ThemeChoice) -> Result<()> {
    let models = selectable_data_models(config)?;

    let mut tui_config = TuiConfig::default();
    tui_config.set_mappings_dir(Some(config.mappings_folder.clone()));
    tui_config.set_schemas_dir(Some(config.schemas_folder.clone()));
    tui_config.set_theme(theme.theme());

    let schema_provider = create_schema_provider(&config.schemas_folder);
    ExplorerBuilder::new(tui_config, models, schema_provider).run()?;

    Ok(())
}
