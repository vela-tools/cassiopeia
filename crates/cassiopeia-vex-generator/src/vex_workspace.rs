//! The workspace files the VEX document is generated from, and the document itself, each named relative
//! to the workspace root.

use crate::{
    advisory_ignores::advisory_ignores,
    disagreement::disagreements,
    document_generation::vex_document,
    generator_error::GeneratorError,
    rulings::parse_rulings,
};
use cargo_lock::Lockfile;
use jiff::Timestamp;
use std::{fs, io::ErrorKind, path::PathBuf};
use url::Url;

/// The cargo-deny configuration, whose ignores the document states.
const DENY_CONFIGURATION: &str = "deny.toml";

/// The rulings file.
const RULINGS: &str = ".vex/rulings.toml";

/// The lockfile every ruled crate version must be held by.
const LOCKFILE: &str = "Cargo.lock";

/// The VEX document.
const DOCUMENT: &str = ".vex/cassiopeia.openvex.json";

/// Where the default branch of the repository serves its files, which the document's identifier points into.
const REPOSITORY_FILES: &str = "https://github.com/vela-tools/cassiopeia/blob/main/";

/// The workspace whose VEX document is generated and checked.
#[derive(Debug)]
pub struct VexWorkspace {
    /// The workspace root.
    root: PathBuf,
}

impl VexWorkspace {
    /// The workspace at `root`.
    pub const fn new(root: PathBuf) -> VexWorkspace {
        VexWorkspace { root }
    }

    /// Write the document the rulings generate, dating a change `now`.
    ///
    /// # Errors
    ///
    /// Fails when an input cannot be read, the rulings disagree with `deny.toml` or `Cargo.lock`, or the
    /// document cannot be written.
    pub fn generate(&self, now: Timestamp) -> Result<(), GeneratorError> {
        let generated = self.generated_text(self.checked_in()?.as_deref(), now)?;
        let path = self.root.join(DOCUMENT);
        fs::write(&path, generated).map_err(|source| GeneratorError::Write { path, source })
    }

    /// Check that the checked-in document is the one the rulings generate.
    ///
    /// # Errors
    ///
    /// Fails when an input cannot be read, the rulings disagree with `deny.toml` or `Cargo.lock`, or the
    /// checked-in document is missing or differs from the generated one.
    pub fn check(&self, now: Timestamp) -> Result<(), GeneratorError> {
        let checked_in = self.checked_in()?;
        let generated = self.generated_text(checked_in.as_deref(), now)?;
        match checked_in {
            Some(checked_in) if checked_in == generated => Ok(()),
            Some(_) | None => Err(GeneratorError::Stale {
                path: self.root.join(DOCUMENT),
            }),
        }
    }

    /// The text of the document the rulings generate, carrying over the version and times of `checked_in`.
    /// A checked-in text that is no OpenVEX document carries nothing over.
    fn generated_text(&self, checked_in: Option<&str>, now: Timestamp) -> Result<String, GeneratorError> {
        let ignores = advisory_ignores(&self.read(DENY_CONFIGURATION)?).map_err(|source| GeneratorError::Toml {
            path: self.root.join(DENY_CONFIGURATION),
            source,
        })?;
        let rulings = parse_rulings(&self.read(RULINGS)?).map_err(|source| GeneratorError::Toml {
            path: self.root.join(RULINGS),
            source,
        })?;
        let lockfile_path = self.root.join(LOCKFILE);
        let lockfile = Lockfile::load(&lockfile_path).map_err(|source| GeneratorError::Lockfile { path: lockfile_path, source })?;
        let found = disagreements(&ignores, &rulings, &lockfile);
        if !found.is_empty() {
            return Err(GeneratorError::Disagreements(found.iter().map(ToString::to_string).collect()));
        }
        let existing = checked_in.and_then(|text| serde_json::from_str(text).ok());
        let id = Url::parse(REPOSITORY_FILES)?.join(DOCUMENT)?;
        let document = vex_document(&rulings, &ignores, id, existing, now)?;
        Ok(serde_json::to_string_pretty(&document)? + "\n")
    }

    /// The checked-in document's text, `None` when there is none.
    fn checked_in(&self) -> Result<Option<String>, GeneratorError> {
        let path = self.root.join(DOCUMENT);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(source) if source.kind() == ErrorKind::NotFound => Ok(None),
            Err(source) => Err(GeneratorError::Read { path, source }),
        }
    }

    /// The text of the workspace file at `relative`.
    fn read(&self, relative: &str) -> Result<String, GeneratorError> {
        let path = self.root.join(relative);
        fs::read_to_string(&path).map_err(|source| GeneratorError::Read { path, source })
    }
}

#[cfg(test)]
mod tests {
    use crate::{generator_error::GeneratorError, vex_workspace::VexWorkspace};
    use jiff::Timestamp;
    use std::{fs, path::Path};
    use tempfile::TempDir;

    const DENY: &str = "[advisories]\nignore = [{ id = \"RUSTSEC-2026-0542\", reason = \"humantime 2.4.0 is unmaintained.\" }]\n";

    const RULINGS: &str = "[RUSTSEC-2026-0542]\njustification = \"vulnerable_code_not_present\"\ncrates = [\"humantime@2.4.0\"]\n";

    const LOCKFILE: &str = "version = 4\n\n[[package]]\nname = \"humantime\"\nversion = \"2.4.0\"\n";

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).unwrap()
    }

    /// A workspace holding the inputs above and the document they generate.
    fn generated_workspace() -> (TempDir, VexWorkspace) {
        let directory = TempDir::new().unwrap();
        fs::create_dir(directory.path().join(".vex")).unwrap();
        fs::write(directory.path().join("deny.toml"), DENY).unwrap();
        fs::write(directory.path().join(".vex/rulings.toml"), RULINGS).unwrap();
        fs::write(directory.path().join("Cargo.lock"), LOCKFILE).unwrap();
        let workspace = VexWorkspace::new(directory.path().to_path_buf());
        workspace.generate(at(1_000)).unwrap();
        (directory, workspace)
    }

    fn rewrite(root: &Path, relative: &str, from: &str, to: &str) {
        let path = root.join(relative);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(path, text.replace(from, to)).unwrap();
    }

    #[test]
    fn a_workspace_holding_the_generated_document_passes_later_on() {
        let (_directory, workspace) = generated_workspace();
        workspace.check(at(2_000)).unwrap();
    }

    #[test]
    fn generating_again_without_a_change_writes_the_same_text() {
        let (directory, workspace) = generated_workspace();
        let first = fs::read_to_string(directory.path().join(".vex/cassiopeia.openvex.json")).unwrap();
        workspace.generate(at(2_000)).unwrap();
        assert_eq!(fs::read_to_string(directory.path().join(".vex/cassiopeia.openvex.json")).unwrap(), first);
    }

    #[test]
    fn a_document_that_does_not_match_the_generated_one_fails() {
        let (directory, workspace) = generated_workspace();
        rewrite(
            directory.path(),
            "deny.toml",
            "is unmaintained.",
            "is unmaintained, and no release replaces it.",
        );
        assert!(matches!(workspace.check(at(2_000)), Err(GeneratorError::Stale { .. })));
    }

    #[test]
    fn a_missing_document_fails() {
        let (directory, workspace) = generated_workspace();
        fs::remove_file(directory.path().join(".vex/cassiopeia.openvex.json")).unwrap();
        assert!(matches!(workspace.check(at(2_000)), Err(GeneratorError::Stale { .. })));
    }

    #[test]
    fn an_ignore_without_a_ruling_fails() {
        let (directory, workspace) = generated_workspace();
        fs::write(directory.path().join(".vex/rulings.toml"), "").unwrap();
        let Err(GeneratorError::Disagreements(found)) = workspace.check(at(2_000)) else {
            panic!("the check passed without a ruling");
        };
        assert_eq!(found, ["deny.toml ignores RUSTSEC-2026-0542, but .vex/rulings.toml has no ruling on it"]);
    }

    #[test]
    fn a_ruling_on_a_crate_version_the_lockfile_does_not_hold_fails() {
        let (directory, workspace) = generated_workspace();
        rewrite(directory.path(), "Cargo.lock", "2.4.0", "2.4.1");
        let Err(GeneratorError::Disagreements(found)) = workspace.check(at(2_000)) else {
            panic!("the check passed with the crate version unlocked");
        };
        assert_eq!(
            found,
            ["the ruling on RUSTSEC-2026-0542 names humantime@2.4.0, but Cargo.lock holds no such crate version"]
        );
    }
}
