//! Generates the QUDT unit table of `cassiopeia-unit-notation` from a release of the QUDT unit vocabulary.
//!
//! QUDT publishes each release as `qudt-public-repo-<release>.zip` on
//! <https://github.com/qudt/qudt-public-repo/releases>; the vocabulary is `vocab/unit/VOCAB_QUDT-UNITS-ALL.ttl`
//! inside it. Extract that file and run:
//!
//! ```text
//! cargo run -p cassiopeia-unit-notation-generator -- generate <path to VOCAB_QUDT-UNITS-ALL.ttl>
//! cargo run -p cassiopeia-unit-notation-generator -- check <path to VOCAB_QUDT-UNITS-ALL.ttl>
//! ```
//!
//! `generate` rewrites `crates/cassiopeia-unit-notation/src/qudt/generated.rs`; `check` fails when that file
//! differs from what the vocabulary generates. The table records the release and the SHA-256 of the file
//! it came from, so a check against another release fails too. After a new release, the tests of
//! `cassiopeia-unit-notation` say which of its corrections, rulings, and supplements the release has made
//! stale.

mod generator_error;
mod qudt_release;
mod qudt_unit_name;
mod qudt_vocabulary;
mod rust_source;
mod unit_notations;

use crate::{generator_error::GeneratorError, qudt_vocabulary::QudtVocabulary, rust_source::rust_source};
use clap::{Parser, Subcommand};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

/// The command line of the generator.
#[derive(Debug, Parser)]
#[command(about = "Generates the QUDT unit table of cassiopeia-unit-notation from the QUDT unit vocabulary")]
struct Arguments {
    /// What to do with the vocabulary.
    #[command(subcommand)]
    command: Command,
}

/// What the generator does with the vocabulary.
#[derive(Debug, Subcommand)]
enum Command {
    /// Write the table the vocabulary generates.
    Generate {
        /// The vocabulary file, `VOCAB_QUDT-UNITS-ALL.ttl`.
        vocabulary: PathBuf,
    },
    /// Fail when the committed table differs from what the vocabulary generates.
    Check {
        /// The vocabulary file, `VOCAB_QUDT-UNITS-ALL.ttl`.
        vocabulary: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(Arguments::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Carries out `command`.
fn run(command: Command) -> Result<(), GeneratorError> {
    let table = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cassiopeia-unit-notation/src/qudt/generated.rs");
    match command {
        Command::Generate { vocabulary } => {
            let source = generated_source(&vocabulary)?;
            fs::write(&table, source).map_err(|source| GeneratorError::Write { path: table, source })
        }
        Command::Check { vocabulary } => {
            let generated = generated_source(&vocabulary)?;
            match fs::read_to_string(&table) {
                Ok(committed) if committed == generated => Ok(()),
                Ok(_) => Err(GeneratorError::Stale { path: table }),
                Err(source) => Err(GeneratorError::Read { path: table, source }),
            }
        }
    }
}

/// The Rust source of the table the vocabulary file at `path` generates.
fn generated_source(path: &Path) -> Result<String, GeneratorError> {
    let bytes = fs::read(path).map_err(|source| GeneratorError::Read { path: path.to_owned(), source })?;
    let vocabulary = QudtVocabulary::parse(&bytes)?;
    for (unit, codes) in vocabulary.unknown_codes() {
        eprintln!("skipped {unit}: Recommendation No. 20 lists none of {codes:?}");
    }
    Ok(rust_source(&vocabulary, &bytes))
}
