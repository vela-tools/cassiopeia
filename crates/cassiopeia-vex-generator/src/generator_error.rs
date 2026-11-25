//! Why the VEX document cannot be generated or checked.

use std::{io, path::PathBuf};
use thiserror::Error;
use toml::de::Error as TomlError;

/// Why the generator failed.
#[derive(Debug, Error)]
pub enum GeneratorError {
    /// A workspace file cannot be read.
    #[error("{} cannot be read: {source}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// Why it cannot be read.
        source: io::Error,
    },
    /// `deny.toml` or the rulings file is not what the generator reads.
    #[error("{} cannot be read as TOML: {source}", path.display())]
    Toml {
        /// The file.
        path: PathBuf,
        /// Why it was refused.
        source: TomlError,
    },
    /// `Cargo.lock` is no lockfile.
    #[error("{} cannot be read as a lockfile: {source}", path.display())]
    Lockfile {
        /// The file.
        path: PathBuf,
        /// Why it was refused.
        source: cargo_lock::Error,
    },
    /// The rulings disagree with `deny.toml` or `Cargo.lock`.
    #[error("the advisory rulings disagree with deny.toml or Cargo.lock:\n{}", .0.join("\n"))]
    Disagreements(Vec<String>),
    /// The checked-in document is not the one the rulings generate.
    #[error("{} is not the document the rulings generate: run `cargo run -p cassiopeia-vex-generator -- generate` and commit the result", path.display())]
    Stale {
        /// The document.
        path: PathBuf,
    },
    /// The document cannot be written.
    #[error("{} cannot be written: {source}", path.display())]
    Write {
        /// The document.
        path: PathBuf,
        /// Why it cannot be written.
        source: io::Error,
    },
    /// A crate version cannot be spelt as a package URL.
    #[error("a crate version cannot be spelt as a package URL: {0}")]
    PackageUrl(#[from] packageurl::Error),
    /// A URL the document names cannot be formed.
    #[error("a URL the document names cannot be formed: {0}")]
    Url(#[from] url::ParseError),
    /// The clock reads an instant the document cannot carry.
    #[error("the clock reads an instant the document cannot carry: {0}")]
    Clock(#[from] jiff::Error),
    /// The document cannot be written as JSON.
    #[error("the document cannot be written as JSON: {0}")]
    Json(#[from] serde_json::Error),
}
