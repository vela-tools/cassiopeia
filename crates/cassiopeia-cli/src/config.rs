use cassiopeia_configuration::download::Download;
use std::path::PathBuf;

/// The settings the CLI command handlers need: where schemas and mappings live, and how the schema
/// downloader should pace itself.
pub struct CliConfig {
    pub schemas_folder: PathBuf,
    pub mappings_folder: PathBuf,
    pub download: Download,
}
