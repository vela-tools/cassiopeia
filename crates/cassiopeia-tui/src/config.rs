use crate::theme::Theme;
use getset::{Getters, Setters};
use std::path::PathBuf;

/// The launch-time settings a screen needs: which palette to render with, and where mappings and
/// schemas live on disk.
#[derive(Debug, Clone, Default, Getters, Setters)]
#[getset(get = "pub", set = "pub")]
pub struct TuiConfig {
    /// The palette the screen renders with.
    theme: Theme,
    /// The directory finished mappings are written to, when configured.
    mappings_dir: Option<PathBuf>,
    /// The directory schemas are read from, when configured.
    schemas_dir: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use crate::{config::TuiConfig, theme::LIGHT};
    use std::path::PathBuf;

    #[test]
    fn setters_and_getters_round_trip() {
        let mut config = TuiConfig::default();
        config.set_theme(LIGHT);
        config.set_schemas_dir(Some(PathBuf::from("/schemas")));
        assert_eq!(config.theme().bg_main, LIGHT.bg_main);
        assert_eq!(config.schemas_dir().as_deref(), Some(PathBuf::from("/schemas").as_path()));
    }
}
