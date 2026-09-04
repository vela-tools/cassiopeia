use crate::error::{CliError, IoAction, Result};
use json5format::{FormatOptions, Json5Format, ParsedDocument};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    str::from_utf8,
};

/// Where the formatted mapping went: rewritten in place, or printed to stdout.
///
/// The two land differently at the dispatch boundary: the in-place path warrants a confirmation
/// report, while the stdout form is itself the command's result and needs no extra chrome.
#[derive(Debug, Clone)]
pub enum FormatOutcome {
    /// The file at this path was formatted in place.
    InPlace(PathBuf),
    /// The formatted document was written to stdout.
    ToStdout,
}

/// Reads a JSON5 mapping file into a parsed document, rejecting a wrong extension.
fn parse_mapping(mapping: PathBuf) -> Result<ParsedDocument> {
    if mapping.extension().unwrap_or_default() != "json5" {
        return Err(CliError::WrongMappingFileExtension { path: mapping });
    }

    let mut buffer = String::new();
    let mut file = File::open(&mapping).map_err(|source| CliError::FileOperation {
        source,
        path: mapping.clone(),
        action: IoAction::Open,
    })?;

    file.read_to_string(&mut buffer).map_err(|source| CliError::FileOperation {
        source,
        path: mapping.clone(),
        action: IoAction::Read,
    })?;

    ParsedDocument::from_string(buffer, Some(mapping.to_string_lossy().to_string()))
        .map_err(|source| CliError::ParseDocumentForFormatting { source, path: mapping })
}

/// Formats a parsed document, either overwriting the file in place or printing the result, and
/// reports which of the two happened.
fn format_document(parsed_document: &ParsedDocument, options: FormatOptions, replace: bool) -> Result<FormatOutcome> {
    let format = Json5Format::with_options(options).map_err(CliError::FormatDocument)?;

    let filename = parsed_document.filename().as_ref().ok_or(CliError::GetFilenameFromDocument)?;
    let bytes = format.to_utf8(parsed_document).map_err(CliError::ConvertDocumentToUtf8)?;

    if replace {
        fs::write(filename, bytes).map_err(|source| CliError::FileOperation {
            source,
            path: PathBuf::from(filename),
            action: IoAction::Write,
        })?;
        Ok(FormatOutcome::InPlace(PathBuf::from(filename)))
    } else {
        print!("{}", from_utf8(&bytes).map_err(CliError::ParseDocumentAsUtf8)?);
        Ok(FormatOutcome::ToStdout)
    }
}

/// Pretty-formats a JSON5 mapping file with the project's conventions (4-space indent, trailing
/// commas), overwriting it when `replace` is set and printing it otherwise, and reports which
/// happened.
///
/// # Errors
///
/// Returns [`CliError`](crate::error::CliError) when the document cannot be parsed, formatted, or
/// written back.
pub fn format_mapping(mapping: &Path, replace: bool) -> Result<FormatOutcome> {
    let parsed_document = parse_mapping(mapping.to_path_buf())?;
    let options = FormatOptions {
        indent_by: 4,
        trailing_commas: true,
        ..Default::default()
    };

    format_document(&parsed_document, options, replace)
}
