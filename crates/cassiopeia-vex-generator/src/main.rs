//! Generates `.vex/cassiopeia.openvex.json`, the OpenVEX document that tells scanners which advisories
//! `deny.toml` ignores do not affect the Cassiopeia image, from those ignores and `.vex/rulings.toml`.
//!
//! ```text
//! cargo run -p cassiopeia-vex-generator -- generate
//! cargo run -p cassiopeia-vex-generator -- check
//! ```
//!
//! `generate` rewrites the document. `check` fails when the document differs from the one the rulings
//! generate, when an ignore has no ruling or no reason, when a ruling names an advisory `deny.toml` does
//! not ignore, and when a ruling names a crate version `Cargo.lock` does not hold. Both refuse to run while
//! the rulings disagree with `deny.toml` or `Cargo.lock`.

mod advisory_ignores;
mod crate_version;
mod disagreement;
mod document_generation;
mod generator_error;
mod rulings;
mod vex_document;
mod vex_workspace;
mod vulnerability_id;

use crate::{generator_error::GeneratorError, vex_workspace::VexWorkspace};
use clap::{Parser, Subcommand};
use jiff::Timestamp;
use std::{path::Path, process::ExitCode};

/// The command line of the generator.
#[derive(Debug, Parser)]
#[command(about = "Generates the OpenVEX document of the Cassiopeia image from deny.toml and .vex/rulings.toml")]
struct Arguments {
    /// What to do with the document.
    #[command(subcommand)]
    command: Command,
}

/// What the generator does with the document.
#[derive(Debug, Subcommand)]
enum Command {
    /// Write the document the rulings generate.
    Generate,
    /// Fail when the checked-in document differs from the one the rulings generate.
    Check,
}

fn main() -> ExitCode {
    match run(&Arguments::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Carries out `command` on the workspace this generator belongs to.
fn run(command: &Command) -> Result<(), GeneratorError> {
    let workspace = VexWorkspace::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let now = Timestamp::from_second(Timestamp::now().as_second())?;
    match command {
        Command::Generate => workspace.generate(now),
        Command::Check => workspace.check(now),
    }
}
