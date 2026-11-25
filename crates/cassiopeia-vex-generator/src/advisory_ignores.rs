//! The advisories `deny.toml` ignores, read from its `advisories.ignore` list.
//!
//! cargo-deny takes an entry as an advisory id, as a table naming the id and the reason, or as a table
//! naming a crate. Only the first two ignore an advisory, and cargo-deny itself checks the rest of the file.

use crate::vulnerability_id::VulnerabilityId;
use serde::{Deserialize, de::IgnoredAny};
use std::collections::BTreeMap;
use toml::de::Error;

/// The part of `deny.toml` that lists ignored advisories.
#[derive(Debug, Default, Deserialize)]
struct DenyConfiguration {
    /// The `[advisories]` table.
    #[serde(default)]
    advisories: AdvisoriesTable,
}

/// The `[advisories]` table.
#[derive(Debug, Default, Deserialize)]
struct AdvisoriesTable {
    /// The `ignore` list.
    #[serde(default)]
    ignore: Vec<IgnoreEntry>,
}

/// One entry of the `ignore` list.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum IgnoreEntry {
    /// A table naming the advisory and, usually, the reason.
    Advisory {
        /// The ignored advisory.
        id: VulnerabilityId,
        /// Why it is ignored.
        reason: Option<String>,
    },
    /// The advisory id alone, with no reason.
    Bare(VulnerabilityId),
    /// A table naming a crate, which ignores no advisory.
    Crate(IgnoredAny),
}

/// Every advisory the `deny.toml` text `text` ignores, with the reason it records; `None` for an advisory
/// ignored without one.
///
/// # Errors
///
/// Fails when the text is no TOML.
pub fn advisory_ignores(text: &str) -> Result<BTreeMap<VulnerabilityId, Option<String>>, Error> {
    let configuration: DenyConfiguration = toml::from_str(text)?;
    Ok(configuration
        .advisories
        .ignore
        .into_iter()
        .filter_map(|entry| match entry {
            IgnoreEntry::Advisory { id, reason } => Some((id, reason)),
            IgnoreEntry::Bare(id) => Some((id, None)),
            IgnoreEntry::Crate(_) => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::advisory_ignores::advisory_ignores;

    fn ignores(text: &str) -> Vec<(String, Option<String>)> {
        advisory_ignores(text)
            .unwrap()
            .into_iter()
            .map(|(id, reason)| (id.to_string(), reason))
            .collect()
    }

    #[test]
    fn an_ignore_table_gives_its_id_and_reason() {
        assert_eq!(
            ignores("[advisories]\nignore = [{ id = \"RUSTSEC-2026-0542\", reason = \"unmaintained\" }]\n"),
            [("RUSTSEC-2026-0542".to_owned(), Some("unmaintained".to_owned()))]
        );
    }

    #[test]
    fn a_bare_id_is_ignored_without_a_reason() {
        assert_eq!(
            ignores("[advisories]\nignore = [\"RUSTSEC-2026-0542\"]\n"),
            [("RUSTSEC-2026-0542".to_owned(), None)]
        );
    }

    #[test]
    fn a_crate_ignore_names_no_advisory() {
        assert_eq!(
            ignores("[advisories]\nignore = [{ crate = \"humantime@2.4.0\", reason = \"unmaintained\" }]\n"),
            []
        );
    }

    #[test]
    fn a_configuration_without_ignores_ignores_nothing() {
        assert_eq!(ignores("[advisories]\nyanked = \"deny\"\n"), []);
        assert_eq!(ignores("[bans]\nwildcards = \"warn\"\n"), []);
    }
}
