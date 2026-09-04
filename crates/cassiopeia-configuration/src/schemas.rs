use cassiopeia_directories::locations::schemas_dir;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use std::path::PathBuf;

/// Where downloaded Smart Data Models JSON Schemas are kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SmartDefault)]
#[serde(default)]
pub struct Schemas {
    /// The directory the validator reads schemas from and the downloader writes them to.
    ///
    /// Defaults to the schema store under the XDG data directory (or the container data root), so
    /// downloaded schemas land with the installation's other application data.
    #[default(schemas_dir())]
    pub folder: PathBuf,
}

#[cfg(test)]
mod tests {
    use crate::schemas::Schemas;
    use std::path::PathBuf;

    #[test]
    fn the_default_folder_is_named_after_what_it_holds() {
        assert!(Schemas::default().folder.ends_with("schemas"));
    }

    #[test]
    fn reads_an_explicit_folder() {
        let schemas: Schemas = toml::from_str(r#"folder = "/srv/schemas""#).unwrap();

        assert_eq!(schemas.folder, PathBuf::from("/srv/schemas"));
    }
}
