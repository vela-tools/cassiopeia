//! The ways the rulings stop agreeing with `deny.toml` or `Cargo.lock`.

use crate::{crate_version::CrateVersion, rulings::Ruling, vulnerability_id::VulnerabilityId};
use cargo_lock::Lockfile;
use std::collections::BTreeMap;
use strum::Display;

/// A ruling or an advisory ignore that disagrees with `deny.toml` or with `Cargo.lock`. Each names the
/// advisory by the id the ignore names.
#[derive(Debug, PartialEq, Eq, Display)]
pub enum Disagreement<'a> {
    /// `deny.toml` ignores the advisory, and no ruling names it.
    #[strum(to_string = "deny.toml ignores {advisory}, but .vex/rulings.toml has no ruling on it")]
    IgnoreWithoutRuling {
        /// The ignored advisory.
        advisory: &'a VulnerabilityId,
    },
    /// A ruling names an advisory `deny.toml` does not ignore.
    #[strum(to_string = ".vex/rulings.toml rules on {advisory}, but deny.toml does not ignore it")]
    RulingWithoutIgnore {
        /// The ruled advisory.
        advisory: &'a VulnerabilityId,
    },
    /// `deny.toml` ignores the advisory by its bare id, so its statements have no impact statement.
    #[strum(to_string = "deny.toml ignores {advisory} without a reason for its VEX statements to repeat")]
    IgnoreWithoutReason {
        /// The ignored advisory.
        advisory: &'a VulnerabilityId,
    },
    /// `Cargo.lock` holds no such crate version.
    #[strum(to_string = "the ruling on {advisory} names {crate_version}, but Cargo.lock holds no such crate version")]
    CrateVersionNotLocked {
        /// The ruling's advisory.
        advisory: &'a VulnerabilityId,
        /// The crate version the lockfile does not hold.
        crate_version: &'a CrateVersion,
    },
}

/// Every way the advisories `ignores` records, the `rulings` and the crates `lockfile` holds disagree:
/// ignores first, then rulings, each in advisory order.
pub fn disagreements<'a>(
    ignores: &'a BTreeMap<VulnerabilityId, Option<String>>,
    rulings: &'a BTreeMap<VulnerabilityId, Ruling>,
    lockfile: &Lockfile,
) -> Vec<Disagreement<'a>> {
    let of_ignores = ignores.iter().flat_map(|(advisory, reason)| {
        let unruled = (!rulings.contains_key(advisory)).then_some(Disagreement::IgnoreWithoutRuling { advisory });
        let unreasoned = reason.is_none().then_some(Disagreement::IgnoreWithoutReason { advisory });
        unruled.into_iter().chain(unreasoned)
    });
    let of_rulings = rulings.iter().flat_map(|(advisory, ruling)| {
        let unignored = (!ignores.contains_key(advisory)).then_some(Disagreement::RulingWithoutIgnore { advisory });
        let unlocked = ruling
            .crates
            .iter()
            .filter(|crate_version| !is_locked(lockfile, crate_version))
            .map(move |crate_version| Disagreement::CrateVersionNotLocked { advisory, crate_version });
        unignored.into_iter().chain(unlocked)
    });
    of_ignores.chain(of_rulings).collect()
}

/// Whether `lockfile` holds `crate_version`.
fn is_locked(lockfile: &Lockfile, crate_version: &CrateVersion) -> bool {
    lockfile
        .packages
        .iter()
        .any(|package| package.name == crate_version.name && package.version == crate_version.version)
}

#[cfg(test)]
mod tests {
    use crate::{
        advisory_ignores::advisory_ignores,
        disagreement::{Disagreement, disagreements},
        rulings::parse_rulings,
    };
    use cargo_lock::Lockfile;

    const LOCKFILE: &str = "version = 4\n\n[[package]]\nname = \"humantime\"\nversion = \"2.4.0\"\n";

    const RULING: &str = "[RUSTSEC-2026-0542]\njustification = \"vulnerable_code_not_present\"\ncrates = [\"humantime@2.4.0\"]\n";

    const IGNORE: &str = "[advisories]\nignore = [{ id = \"RUSTSEC-2026-0542\", reason = \"humantime 2.4.0 is unmaintained.\" }]\n";

    fn found(deny: &str, rulings: &str, lockfile: &str) -> Vec<String> {
        let ignores = advisory_ignores(deny).unwrap();
        let rulings = parse_rulings(rulings).unwrap();
        let lockfile: Lockfile = lockfile.parse().unwrap();
        disagreements(&ignores, &rulings, &lockfile).iter().map(Disagreement::to_string).collect()
    }

    #[test]
    fn rulings_that_agree_with_deny_toml_and_the_lockfile_raise_nothing() {
        assert_eq!(found(IGNORE, RULING, LOCKFILE), Vec::<String>::new());
    }

    #[test]
    fn an_ignore_without_a_ruling_disagrees() {
        assert_eq!(
            found(IGNORE, "", LOCKFILE),
            ["deny.toml ignores RUSTSEC-2026-0542, but .vex/rulings.toml has no ruling on it"]
        );
    }

    #[test]
    fn a_ruling_without_an_ignore_disagrees() {
        assert_eq!(
            found("", RULING, LOCKFILE),
            [".vex/rulings.toml rules on RUSTSEC-2026-0542, but deny.toml does not ignore it"]
        );
    }

    #[test]
    fn an_ignore_without_a_reason_disagrees() {
        assert_eq!(
            found("[advisories]\nignore = [\"RUSTSEC-2026-0542\"]\n", RULING, LOCKFILE),
            ["deny.toml ignores RUSTSEC-2026-0542 without a reason for its VEX statements to repeat"]
        );
    }

    #[test]
    fn a_crate_version_the_lockfile_does_not_hold_disagrees() {
        assert_eq!(
            found(IGNORE, RULING, &LOCKFILE.replace("2.4.0", "2.4.1")),
            ["the ruling on RUSTSEC-2026-0542 names humantime@2.4.0, but Cargo.lock holds no such crate version"]
        );
    }
}
