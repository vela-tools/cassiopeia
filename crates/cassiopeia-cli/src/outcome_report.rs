use cassiopeia_terminal_style::{
    byte_size::human_size,
    paint::{join, paint},
    palette::{ACCENT, HIGHLIGHT, MUTED, PRIMARY},
    rendering::Rendering,
};
use std::path::{Path, PathBuf};

/// A terminal-final outcome of a one-shot CLI command: an artefact was written or produced and the
/// command is done. One cohesive concept (command completion), so every variant renders through the
/// same compact, `--version`-style two-line report.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// A default configuration file was written.
    ConfigurationWritten { path: PathBuf, size: u64 },
    /// A pipeline manifest file was written.
    ManifestWritten { path: PathBuf, size: u64 },
    /// The command-line reference Markdown was written.
    MarkdownWritten { path: PathBuf, size: u64 },
    /// A JSON5 mapping was converted to plain JSON.
    MappingConverted { path: PathBuf, size: u64 },
    /// A JSON5 mapping was formatted in place.
    MappingFormatted { path: PathBuf },
    /// A JSON Schema was dereferenced; its expanded payload has already gone to stdout.
    SchemaDereferenced { schema: PathBuf },
}

impl Outcome {
    /// The pieces the two-line report is built from: the path whose file name anchors the identity
    /// line, the written size when one is meaningful, and the verb and subject of the detail line.
    fn describe(&self) -> (&Path, Option<u64>, &'static str, &'static str) {
        match self {
            Outcome::ConfigurationWritten { path, size } => (path, Some(*size), "written", "default configuration"),
            Outcome::ManifestWritten { path, size } => (path, Some(*size), "written", "pipeline manifest"),
            Outcome::MarkdownWritten { path, size } => (path, Some(*size), "written", "command reference"),
            Outcome::MappingConverted { path, size } => (path, Some(*size), "converted", "JSON mapping"),
            Outcome::MappingFormatted { path } => (path, None, "formatted", "JSON5 mapping"),
            Outcome::SchemaDereferenced { schema } => (schema, None, "dereferenced", "JSON Schema"),
        }
    }
}

/// Renders a one-shot command outcome as a compact report matching `--version` and `profile`: an
/// identity line (file name, and size when known) over a detail line (the action and its subject),
/// with no leading status symbol.
///
/// Parameterised over [`Rendering`] so both the coloured and plain forms are testable without a
/// terminal.
#[must_use]
pub fn render_outcome(outcome: &Outcome, rendering: Rendering) -> String {
    let (path, size, action, subject) = outcome.describe();

    let mut identity = vec![paint(PRIMARY, &file_name(path), rendering)];
    if let Some(size) = size {
        identity.push(paint(MUTED, &human_size(size), rendering));
    }

    let detail = join(&[paint(ACCENT, action, rendering), paint(HIGHLIGHT, subject, rendering)], rendering);
    format!("{}\n{}", join(&identity, rendering), detail)
}

/// The file name of a path as a display string, falling back to the whole path when it has none.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use crate::outcome_report::{Outcome, render_outcome};
    use cassiopeia_terminal_style::rendering::Rendering;
    use std::path::PathBuf;

    const ESCAPE: char = '\u{1b}';

    #[test]
    fn a_written_configuration_renders_identity_over_detail() {
        let outcome = Outcome::ConfigurationWritten {
            path: PathBuf::from("config.toml"),
            size: 1229,
        };

        assert_eq!(
            render_outcome(&outcome, Rendering::Plain),
            "config.toml \u{b7} 1.2 KiB\nwritten \u{b7} default configuration"
        );
    }

    #[test]
    fn a_written_manifest_names_the_pipeline_manifest() {
        let outcome = Outcome::ManifestWritten {
            path: PathBuf::from("out/manifest.toml"),
            size: 2048,
        };

        assert_eq!(
            render_outcome(&outcome, Rendering::Plain),
            "manifest.toml \u{b7} 2.0 KiB\nwritten \u{b7} pipeline manifest"
        );
    }

    #[test]
    fn written_markdown_names_the_command_reference() {
        let outcome = Outcome::MarkdownWritten {
            path: PathBuf::from("cli.md"),
            size: 512,
        };

        assert_eq!(
            render_outcome(&outcome, Rendering::Plain),
            "cli.md \u{b7} 512 B\nwritten \u{b7} command reference"
        );
    }

    #[test]
    fn a_converted_mapping_names_the_json_mapping() {
        let outcome = Outcome::MappingConverted {
            path: PathBuf::from("mapping.json"),
            size: 4096,
        };

        assert_eq!(
            render_outcome(&outcome, Rendering::Plain),
            "mapping.json \u{b7} 4.0 KiB\nconverted \u{b7} JSON mapping"
        );
    }

    #[test]
    fn a_formatted_mapping_omits_the_size() {
        let outcome = Outcome::MappingFormatted {
            path: PathBuf::from("mapping.json5"),
        };

        assert_eq!(render_outcome(&outcome, Rendering::Plain), "mapping.json5\nformatted \u{b7} JSON5 mapping");
    }

    #[test]
    fn a_dereferenced_schema_omits_the_size() {
        let outcome = Outcome::SchemaDereferenced {
            schema: PathBuf::from("schemas/Building.json"),
        };

        assert_eq!(render_outcome(&outcome, Rendering::Plain), "Building.json\ndereferenced \u{b7} JSON Schema");
    }

    #[test]
    fn a_plain_outcome_carries_no_escape_codes() {
        let outcome = Outcome::ConfigurationWritten {
            path: PathBuf::from("config.toml"),
            size: 1229,
        };

        assert!(!render_outcome(&outcome, Rendering::Plain).contains(ESCAPE));
    }

    #[test]
    fn a_coloured_outcome_carries_escape_codes() {
        let outcome = Outcome::ConfigurationWritten {
            path: PathBuf::from("config.toml"),
            size: 1229,
        };

        assert!(render_outcome(&outcome, Rendering::Colored).contains(ESCAPE));
    }
}
