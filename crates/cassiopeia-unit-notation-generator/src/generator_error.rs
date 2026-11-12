use oxttl::TurtleSyntaxError;
use std::{io, path::PathBuf};
use thiserror::Error;

/// Why the table could not be generated or checked.
#[derive(Debug, Error)]
pub enum GeneratorError {
    /// A file could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// What the operating system reported.
        source: io::Error,
    },
    /// The table could not be written.
    #[error("cannot write {}: {source}", path.display())]
    Write {
        /// The table.
        path: PathBuf,
        /// What the operating system reported.
        source: io::Error,
    },
    /// The vocabulary is not well-formed Turtle.
    #[error("the vocabulary is not well-formed Turtle: {0}")]
    Syntax(#[from] TurtleSyntaxError),
    /// The vocabulary does not state which QUDT release it belongs to.
    #[error("the vocabulary states no release: its ontology has no `owl:versionIRI` of the form `http://qudt.org/<release>/vocab/unit`")]
    NoRelease,
    /// The committed table differs from what the vocabulary generates.
    #[error("{} is stale: run `cargo run -p cassiopeia-unit-notation-generator -- generate` with the same vocabulary", path.display())]
    Stale {
        /// The table.
        path: PathBuf,
    },
}
