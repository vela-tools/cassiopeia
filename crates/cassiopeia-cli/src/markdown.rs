use crate::error::{CliError, IoAction, Result};
use clap::CommandFactory;
use std::{fs::File, io::Write, path::PathBuf};

/// Generates command-line reference documentation for a clap command tree, writing it to a file and
/// returning the path written.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the generated Markdown cannot be written to
/// `output_path`.
pub fn generate_markdown<T: CommandFactory>(output_path: &str) -> Result<PathBuf> {
    let markdown: String = clap_markdown::help_markdown::<T>();

    let mut file = File::create(output_path).map_err(|source| CliError::FileOperation {
        source,
        path: PathBuf::from(output_path),
        action: IoAction::Create,
    })?;

    file.write_all(markdown.as_bytes()).map_err(|source| CliError::FileOperation {
        source,
        path: PathBuf::from(output_path),
        action: IoAction::Write,
    })?;

    Ok(PathBuf::from(output_path))
}
