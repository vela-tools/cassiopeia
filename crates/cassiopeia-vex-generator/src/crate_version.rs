//! One crate at one version, as a ruling names it and as a VEX statement names it.

use cargo_lock::{Name, Version};
use derive_more::Display;
use packageurl::PackageUrl;
use serde_with::DeserializeFromStr;
use std::str::FromStr;
use thiserror::Error;

/// The package URL type of a crate.
const CRATE_TYPE: &str = "cargo";

/// A crate at one version, spelt `<name>@<version>` in the rulings file.
#[derive(Debug, PartialEq, Eq, Display, DeserializeFromStr)]
#[display("{name}@{version}")]
pub struct CrateVersion {
    /// The crate name.
    pub name: Name,
    /// The crate version.
    pub version: Version,
}

/// Why a text names no crate version.
#[derive(Debug, Error)]
pub enum CrateVersionError {
    /// The text has no `@` between a name and a version.
    #[error("`{0}` is not spelt <name>@<version>")]
    NotNameAtVersion(String),
    /// The part before the `@` is no crate name.
    #[error("`{spelt}` names no crate")]
    Name {
        /// The text as the rulings file spells it.
        spelt: String,
        /// Why the name was refused.
        #[source]
        source: cargo_lock::Error,
    },
    /// The part after the `@` is no semantic version.
    #[error("`{spelt}` names no semantic version")]
    Version {
        /// The text as the rulings file spells it.
        spelt: String,
        /// Why the version was refused.
        #[source]
        source: semver::Error,
    },
}

impl FromStr for CrateVersion {
    type Err = CrateVersionError;

    fn from_str(spelt: &str) -> Result<CrateVersion, CrateVersionError> {
        let (name, version) = spelt.split_once('@').ok_or_else(|| CrateVersionError::NotNameAtVersion(spelt.to_owned()))?;
        Ok(CrateVersion {
            name: name.parse().map_err(|source| CrateVersionError::Name {
                spelt: spelt.to_owned(),
                source,
            })?,
            version: version.parse().map_err(|source| CrateVersionError::Version {
                spelt: spelt.to_owned(),
                source,
            })?,
        })
    }
}

impl CrateVersion {
    /// The package URL that names this crate version, such as `pkg:cargo/humantime@2.4.0`.
    ///
    /// # Errors
    ///
    /// Fails when the name or the version cannot be spelt in a package URL.
    pub fn package_url(&self) -> Result<String, packageurl::Error> {
        let mut purl = PackageUrl::new(CRATE_TYPE, self.name.as_str())?;
        purl.with_version(self.version.to_string())?;
        Ok(purl.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::crate_version::{CrateVersion, CrateVersionError};

    #[test]
    fn a_crate_version_reads_its_name_and_version() {
        let crate_version: CrateVersion = "libsqlite3-sys@0.38.2".parse().unwrap();
        assert_eq!(crate_version.name.as_str(), "libsqlite3-sys");
        assert_eq!(crate_version.version.to_string(), "0.38.2");
        assert_eq!(crate_version.to_string(), "libsqlite3-sys@0.38.2");
    }

    #[test]
    fn a_crate_version_is_named_by_a_cargo_package_url() {
        let crate_version: CrateVersion = "humantime@2.4.0".parse().unwrap();
        assert_eq!(crate_version.package_url().unwrap(), "pkg:cargo/humantime@2.4.0");
    }

    #[test]
    fn a_text_without_a_name_at_a_semantic_version_is_refused() {
        assert!(matches!("humantime 2.4.0".parse::<CrateVersion>(), Err(CrateVersionError::NotNameAtVersion(_))));
        assert!(matches!("humantime@2.4".parse::<CrateVersion>(), Err(CrateVersionError::Version { .. })));
    }
}
