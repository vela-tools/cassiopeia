use cassiopeia_directories::locations::cursors_dir;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use std::path::PathBuf;

/// Where the cursor stores of incremental reads are kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SmartDefault)]
#[serde(default)]
pub struct Cursors {
    /// The directory holding one store per cursor, each named after its cursor.
    ///
    /// Defaults to the cursors folder under the XDG data directory (or the container data root),
    /// since a cursor keeps what runs have read, which the next run resumes from.
    #[default(cursors_dir())]
    pub folder: PathBuf,
}

#[cfg(test)]
mod tests {
    use crate::cursors::Cursors;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    #[test]
    fn the_default_folder_is_named_after_what_it_holds() {
        assert!(Cursors::default().folder.ends_with("cursors"));
    }

    #[test]
    fn reads_an_explicit_folder() {
        let cursors: Cursors = toml::from_str(r#"folder = "/srv/cursors""#).unwrap();

        assert_eq!(cursors.folder, PathBuf::from("/srv/cursors"));
    }
}
