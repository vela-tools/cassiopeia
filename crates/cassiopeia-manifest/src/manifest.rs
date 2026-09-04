use crate::{
    error::{ManifestError, Result},
    failure_policy::FailurePolicy,
    inputs::Inputs,
    output::ManifestOutput,
    schedule::Schedule,
    version::Version,
};
use cassiopeia_common::{
    error::io::{IoAction, IoError},
    memory_profile::MemoryProfile,
};
use getset::Getters;
use json5format::{FormatOptions, Json5Format, ParsedDocument};
use serde::{Deserialize, Serialize};
use std::{
    fs::{read_to_string, write},
    path::Path,
};
use typed_builder::TypedBuilder;

/// How far a written manifest is indented.
const INDENT: usize = 4;

/// A run described in full: which sources are read, when they are read, and where the entities go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters, TypedBuilder)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    /// The document version this manifest is written against.
    #[getset(get = "pub")]
    version: Version,

    /// The sources the run reads.
    #[getset(get = "pub")]
    inputs: Inputs,

    /// How the run reacts to a failed cycle and what it exits with: abort, continue, or ignore, for
    /// one-shot and scheduled runs alike.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    on_failure: Option<FailurePolicy>,

    /// When the run repeats. Absent means the run happens once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    schedule: Option<Schedule>,

    /// Where the produced entities go. Absent leaves the destination to the invoking command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    output: Option<ManifestOutput>,

    /// A memory profile for this manifest, overriding the one the invoking command resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    memory_profile: Option<MemoryProfile>,

    /// Run-level variables visible to every mapping in the run as `{{ vars.<name> }}`.
    ///
    /// A global constant every input's records can read (a dataset id, a run stamp, a validity
    /// instant), authored once here rather than pasted as a literal into each mapping. A per-input
    /// [`vars`](crate::input::ManifestInput::vars) map overrides a global of the same name, and a
    /// `--var` on the command line overrides the manifest. Values are arbitrary JSON; a non-string
    /// value keeps its type only through a bare `{{ vars.x }}` and stringifies in any concatenation or
    /// filter, the same rule as every other source field. `vars` is reserved in the record namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    vars: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Manifest {
    /// Reads a manifest from a JSON5 document on disk.
    ///
    /// # Errors
    /// Returns [`ManifestError::Io`] if the file cannot be read, or a parse error via
    /// [`Manifest::from_json5`].
    pub fn from_file(path: &Path) -> Result<Manifest> {
        let content = read_to_string(path).map_err(|source| IoError::FileOperation {
            source,
            path: path.to_path_buf(),
            action: IoAction::Read,
        })?;

        Manifest::from_json5(&content, path)
    }

    /// Reads a manifest from a JSON5 document, naming `origin` in any parse failure.
    ///
    /// # Errors
    /// Returns [`ManifestError::Parse`] if `content` is not a valid manifest document.
    pub fn from_json5(content: &str, origin: &Path) -> Result<Manifest> {
        serde_json5::from_str(content).map_err(|source| ManifestError::Parse {
            path: origin.to_path_buf(),
            source,
        })
    }

    /// Renders the manifest as an indented JSON5 document.
    fn to_json5(&self) -> Result<String> {
        let raw = serde_json::to_string(self)?;
        let parsed = ParsedDocument::from_str(&raw, None).map_err(|error| ManifestError::Format { message: error.to_string() })?;
        let options = FormatOptions {
            indent_by: INDENT,
            trailing_commas: true,
            ..Default::default()
        };
        let format = Json5Format::with_options(options).map_err(|error| ManifestError::Format { message: error.to_string() })?;
        let bytes = format.to_utf8(&parsed).map_err(|error| ManifestError::Format { message: error.to_string() })?;

        Ok(String::from_utf8(bytes)?)
    }

    /// Writes the manifest to disk as an indented JSON5 document.
    ///
    /// # Errors
    /// Returns [`ManifestError::Format`] or [`ManifestError::Serialize`] if the manifest cannot be
    /// rendered, or [`ManifestError::Io`] if it cannot be written.
    pub fn write_to_file(&self, path: &Path) -> Result<()> {
        let content = self.to_json5()?;

        write(path, content).map_err(|source| {
            IoError::FileOperation {
                source,
                path: path.to_path_buf(),
                action: IoAction::Write,
            }
            .into()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        failure_policy::FailurePolicy,
        manifest::Manifest,
        output::{ManifestOutput, destination::Destination},
        schedule::{Schedule, trigger::Trigger},
        version::Version,
    };
    use std::{path::Path, time::Duration};

    const MINIMAL: &str = r#"{
        version: "v1",
        inputs: [
            { source: "data/stations.csv", mapping: "mappings/station.json5" },
        ],
    }"#;

    #[test]
    fn reads_a_manifest_stating_only_a_version_and_one_input() {
        let manifest = Manifest::from_json5(MINIMAL, Path::new("manifest.json5")).unwrap();

        assert_eq!(manifest.version(), &Version::V1);
        assert_eq!(manifest.inputs().len(), 1);
        assert_eq!(manifest.schedule(), &None);
        assert_eq!(manifest.output(), &None);
    }

    #[test]
    fn reads_a_manifest_stating_a_schedule_and_an_output() {
        let manifest = Manifest::from_json5(
            r#"{
                version: "v1",
                onFailure: "abort",
                inputs: [{ source: "https://example.org/a.json", mapping: "m.json5", format: "json" }],
                schedule: { mode: "every", value: "15m" },
                output: { target: "context-broker", url: "http://localhost:1026/", contextDelivery: "link-header" },
                memoryProfile: "low-memory",
            }"#,
            Path::new("manifest.json5"),
        )
        .unwrap();

        assert_eq!(manifest.on_failure(), &Some(FailurePolicy::Abort));
        assert_eq!(
            manifest.schedule().as_ref().map(Schedule::trigger),
            Some(&Trigger::Every(Duration::from_mins(15)))
        );
        assert!(matches!(
            manifest.output().as_ref().map(ManifestOutput::destination),
            Some(&Destination::ContextBroker { .. })
        ));
    }

    #[test]
    fn a_manifest_with_no_inputs_is_rejected() {
        assert!(Manifest::from_json5(r#"{ version: "v1", inputs: [] }"#, Path::new("manifest.json5")).is_err());
    }

    #[test]
    fn a_manifest_with_an_unrecognised_field_is_rejected() {
        assert!(Manifest::from_json5(r#"{ version: "v1", inputs: [], failFast: true }"#, Path::new("manifest.json5")).is_err());
    }

    #[test]
    fn a_manifest_round_trips_through_its_json5_rendering() {
        let manifest = Manifest::from_json5(MINIMAL, Path::new("manifest.json5")).unwrap();
        let rendered = manifest.to_json5().unwrap();

        assert_eq!(Manifest::from_json5(&rendered, Path::new("manifest.json5")).unwrap(), manifest);
    }

    #[test]
    fn reads_a_manifest_stating_global_and_per_input_vars() {
        let manifest = Manifest::from_json5(
            r#"{
                version: "v1",
                vars: { valid_from: "2026-08-04T16:00:00Z", provider: "SenLab" },
                inputs: [
                    { source: "a.csv", mapping: "m.json5", vars: { provider: "Override" } },
                ],
            }"#,
            Path::new("manifest.json5"),
        )
        .unwrap();

        let global = manifest.vars().as_ref().unwrap();
        assert_eq!(global.get("valid_from"), Some(&serde_json::json!("2026-08-04T16:00:00Z")));
        assert_eq!(global.get("provider"), Some(&serde_json::json!("SenLab")));

        let per_input = manifest.inputs().iter().next().unwrap().vars().as_ref().unwrap();
        assert_eq!(per_input.get("provider"), Some(&serde_json::json!("Override")));
    }

    #[test]
    fn a_manifest_with_vars_round_trips_through_its_json5_rendering() {
        let manifest = Manifest::from_json5(
            r#"{
                version: "v1",
                vars: { valid_from: "2026-08-04T16:00:00Z" },
                inputs: [{ source: "a.csv", mapping: "m.json5", vars: { run: 7 } }],
            }"#,
            Path::new("manifest.json5"),
        )
        .unwrap();
        let rendered = manifest.to_json5().unwrap();

        assert_eq!(Manifest::from_json5(&rendered, Path::new("manifest.json5")).unwrap(), manifest);
    }
}
