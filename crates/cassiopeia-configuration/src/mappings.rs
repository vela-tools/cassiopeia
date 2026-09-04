use cassiopeia_directories::locations::mappings_dir;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use std::path::PathBuf;

/// Where the mapping documents a run can refer to by name are kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SmartDefault)]
#[serde(default)]
pub struct Mappings {
    /// The directory holding the user's own mapping documents.
    ///
    /// Defaults to the mapping folder under the XDG config directory (or the container config
    /// root), since a mapping is user-authored configuration rather than downloaded data.
    #[default(mappings_dir())]
    pub folder: PathBuf,
}

#[cfg(test)]
mod tests {
    use crate::mappings::Mappings;
    use std::path::PathBuf;

    #[test]
    fn mappings_default_to_the_xdg_config_mappings_folder() {
        assert!(Mappings::default().folder.ends_with("cassiopeia/mappings"));
    }

    #[test]
    fn reads_an_explicit_folder() {
        let mappings: Mappings = toml::from_str(r#"folder = "/srv/mappings""#).unwrap();

        assert_eq!(mappings.folder, PathBuf::from("/srv/mappings"));
    }
}
