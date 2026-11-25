//! The rulings file, `.vex/rulings.toml`: one table per advisory `deny.toml` ignores, named by the id the
//! ignore names, giving the advisory's other identifiers, the OpenVEX justification and the crate versions
//! the ruling is about.
//!
//! The reason of a ruling is the ignore's own, so the file never repeats it.

use crate::{crate_version::CrateVersion, vex_document::Justification, vulnerability_id::VulnerabilityId};
use serde::Deserialize;
use std::collections::BTreeMap;
use toml::de::Error;

/// What the rulings file records about one advisory.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ruling {
    /// The advisory's other identifiers, such as its CVE and GHSA ids; empty when it has none.
    #[serde(default)]
    pub aliases: Vec<VulnerabilityId>,
    /// Why the image is not affected.
    pub justification: Justification,
    /// The crate versions the ruling is about.
    pub crates: Vec<CrateVersion>,
}

/// Every ruling the rulings file `text` records, by the advisory id it is named by.
///
/// # Errors
///
/// Fails when the text is no TOML, or a table is no ruling.
pub fn parse_rulings(text: &str) -> Result<BTreeMap<VulnerabilityId, Ruling>, Error> {
    toml::from_str(text)
}

#[cfg(test)]
mod tests {
    use crate::{rulings::parse_rulings, vex_document::Justification};

    #[test]
    fn a_ruling_reads_its_aliases_justification_and_crates() {
        let rulings = parse_rulings(
            "[RUSTSEC-2026-0459]\naliases = [\"CVE-2026-119375\", \"GHSA-72w2-rjgg-q729\"]\njustification = \"vulnerable_code_not_present\"\ncrates = [\"libsqlite3-sys@0.38.2\"]\n",
        )
        .unwrap();
        let (advisory, ruling) = rulings.iter().next().unwrap();
        assert_eq!(advisory.to_string(), "RUSTSEC-2026-0459");
        assert_eq!(
            ruling.aliases.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["CVE-2026-119375", "GHSA-72w2-rjgg-q729"]
        );
        assert_eq!(ruling.justification, Justification::VulnerableCodeNotPresent);
        assert_eq!(ruling.crates[0].to_string(), "libsqlite3-sys@0.38.2");
    }

    #[test]
    fn a_ruling_without_aliases_has_none() {
        let rulings = parse_rulings("[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error@1.0.4\"]\n").unwrap();
        assert!(rulings.values().all(|ruling| ruling.aliases.is_empty()));
    }

    #[test]
    fn a_table_that_is_no_ruling_is_refused() {
        for text in [
            "[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error@1.0.4\"]\nreason = \"build-time only\"\n",
            "[RUSTSEC-2024-0370]\njustification = \"unreachable\"\ncrates = [\"proc-macro-error@1.0.4\"]\n",
            "[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error 1.0.4\"]\n",
            "[RUSTSEC-2024-0370]\ncrates = [\"proc-macro-error@1.0.4\"]\n",
        ] {
            assert!(parse_rulings(text).is_err(), "{text}");
        }
    }
}
