use crate::{error::Result, profile_report::render_profile};
use cassiopeia_data_profiler::profile_path;
use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
use cassiopeia_terminal_style::rendering::{Stream, detect_rendering};
use std::{fs::metadata, path::Path};

/// Detects the source format of `file` and prints a compact profile report to stdout.
///
/// The report is written to stdout (not the reporter) so it is captured intact when redirected, and
/// coloured only for an interactive terminal.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the file's format cannot be detected, or when
/// its profile cannot be rendered.
pub fn profile_file(file: &Path) -> Result<()> {
    // A local file carries no source context, so its encoding is read from its content alone.
    let profile = profile_path(file, &EncodingHint::NONE)?;
    let name = file
        .file_name()
        .map_or_else(|| file.display().to_string(), |name| name.to_string_lossy().into_owned());
    let size = metadata(file).map(|meta| meta.len()).ok();
    println!("{}", render_profile(&name, size, &profile, detect_rendering(Stream::Stdout))?);

    Ok(())
}
