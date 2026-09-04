use crate::{
    config::CliConfig,
    error::Result,
    model_selection::selectable_data_models,
    schema::{create_saver, create_schema_provider},
    theme_choice::ThemeChoice,
};
use cassiopeia_tui::{config::TuiConfig, screen::wizard::builder::WizardBuilder};

/// Launches the mapping wizard, writing a finished mapping into the configured mappings folder.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the catalog store cannot be opened or the
/// wizard fails to produce a mapping.
pub fn launch_wizard(config: &CliConfig, theme: ThemeChoice) -> Result<()> {
    let models = selectable_data_models(config)?;

    let mut tui_config = TuiConfig::default();
    tui_config.set_mappings_dir(Some(config.mappings_folder.clone()));
    tui_config.set_schemas_dir(Some(config.schemas_folder.clone()));
    tui_config.set_theme(theme.theme());

    let schema_provider = create_schema_provider(&config.schemas_folder);
    let saver = create_saver(&config.mappings_folder);
    WizardBuilder::new(tui_config, models, schema_provider, saver).run()?;

    Ok(())
}
