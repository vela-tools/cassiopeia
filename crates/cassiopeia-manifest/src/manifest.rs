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
    /// `--var` on the command line overrides the manifest. Values are arbitrary JSON and keep their
    /// type, read by the same rules as a source field. They are read beside each record rather than
    /// inside it, so a template reaching the whole record never sees them, and a source field called
    /// `vars` stays reachable as `this['vars']`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = None)]
    #[getset(get = "pub")]
    vars: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Manifest {
    /// Reads a manifest from a JSON5 document on disk.
    ///
    /// Every relative local path the manifest names is resolved against the manifest's own
    /// directory, as [`Manifest::resolved_against`] describes, so the manifest runs the same from any
    /// working directory.
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
        // A bare file name has an empty parent, which joins to the working directory it lives in.
        let directory = path.parent().unwrap_or(Path::new(""));

        Ok(Manifest::from_json5(&content, path)?.resolved_against(directory))
    }

    /// Resolves every relative local path the manifest names against `directory`: each input's
    /// `source`, mappings and `context`, and the output's `directory`, `context` and validation
    /// `schema` and `report`. Absolute paths and URLs are kept as they are, and a per-input `schema`
    /// keeps resolving next to its mapping.
    #[must_use]
    pub fn resolved_against(self, directory: &Path) -> Manifest {
        Manifest {
            inputs: self.inputs.resolved_against(directory),
            output: self.output.map(|output| output.resolved_against(directory)),
            ..self
        }
    }

    /// Reads a manifest from a JSON5 document, naming `origin` in any parse failure.
    ///
    /// Every `${NAME}` in an input's `source` or `headers`, or in the output's `url` or `headers`, is
    /// filled from this process's environment, and each value read is recorded as a secret every
    /// output sink masks. No other field reads the environment.
    ///
    /// # Errors
    /// Returns [`ManifestError::Parse`] if `content` is not a valid manifest document, including when
    /// a field names an environment variable that is not set.
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
        error::ManifestError,
        failure_policy::FailurePolicy,
        manifest::Manifest,
        mapping_binding::{CollectionMapping, MappingBinding},
        output::{ManifestOutput, destination::Destination},
        schedule::{Schedule, trigger::Trigger},
        version::Version,
    };
    use cassiopeia_common::{
        collection::CollectionName,
        context::mode::AtContextMode,
        environment::registry::mask,
        file_framing::FileFraming,
        input::Input,
        masked_url::MaskedUrl,
        request_header::RequestHeaders,
        schema_source::SchemaSource,
    };
    use pretty_assertions::assert_eq;
    use std::{
        fs::{create_dir, write},
        path::{Path, PathBuf},
        time::Duration,
    };
    use tempfile::TempDir;

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

    /// What the JSON5 parser objected to in a manifest that failed to parse.
    fn parse_failure(document: &str) -> String {
        match Manifest::from_json5(document, Path::new("manifest.json5")) {
            Err(ManifestError::Parse { source, .. }) => source.to_string(),
            other => panic!("expected a parse failure, got {other:?}"),
        }
    }

    #[test]
    fn a_manifest_whose_output_carries_the_retired_skip_null_key_fails_to_parse_with_an_unknown_field_error() {
        let message = parse_failure(
            r#"{
                version: "v1",
                inputs: [{ source: "data/stations.csv", mapping: "mappings/station.json5" }],
                output: { target: "file", directory: "out", skipNull: "include" },
            }"#,
        );

        assert!(message.contains("unknown field `skipNull`"), "{message}");
    }

    #[test]
    fn a_manifest_whose_output_validation_carries_the_retired_skip_null_key_fails_to_parse_with_an_unknown_field_error() {
        let message = parse_failure(
            r#"{
                version: "v1",
                inputs: [{ source: "data/stations.csv", mapping: "mappings/station.json5" }],
                output: { target: "file", directory: "out", validation: { mode: "fail", skipNull: "skip" } },
            }"#,
        );

        assert!(message.contains("unknown field `skipNull`"), "{message}");
    }

    /// A manifest naming a relative path in every field that resolves against the manifest's directory,
    /// plus a per-input `schema`, which resolves next to its mapping instead.
    const RELATIVE_PATHS: &str = r#"{
        version: "v1",
        inputs: [
            { source: "data/a.csv", mapping: "m.json5", context: "ctx.jsonld", schema: "a.schema.json" },
            { source: "https://example.org/b.json", mappings: [{ collection: "B", mapping: "/srv/b.json5" }] },
        ],
        output: {
            target: "file",
            directory: "out",
            context: "out.jsonld",
            validation: { schema: "g.schema.json", report: "report.json" },
        },
    }"#;

    #[test]
    fn a_manifest_read_from_a_file_resolves_its_relative_paths_against_the_manifest_directory() {
        let root = TempDir::new().unwrap();
        let directory = root.path().join("run");
        create_dir(&directory).unwrap();
        let path = directory.join("run.json5");
        write(&path, RELATIVE_PATHS).unwrap();

        let manifest = Manifest::from_file(&path).unwrap();
        let mut inputs = manifest.inputs().iter();
        let local = inputs.next().unwrap();
        let remote = inputs.next().unwrap();
        let output = manifest.output().as_ref().unwrap();
        let validation = output.validation().as_ref().unwrap();

        assert_eq!(local.source(), &Input::Local(directory.join("data/a.csv")));
        assert_eq!(
            local.mapping_binding(),
            &MappingBinding::Single {
                mapping: directory.join("m.json5")
            }
        );
        assert_eq!(local.context(), &Some(AtContextMode::Local(directory.join("ctx.jsonld"))));
        assert_eq!(local.schema(), &Some(SchemaSource::Local(PathBuf::from("a.schema.json"))));
        assert_eq!(remote.source().to_string(), "https://example.org/b.json");
        assert_eq!(
            remote.mapping_binding(),
            &MappingBinding::Collections {
                mappings: vec![CollectionMapping::new(CollectionName::from("B"), PathBuf::from("/srv/b.json5"))],
            }
        );
        assert_eq!(output.destination(), &Destination::file(Some(directory.join("out")), FileFraming::Array));
        assert_eq!(output.context(), &Some(AtContextMode::Local(directory.join("out.jsonld"))));
        assert_eq!(validation.schema(), &Some(SchemaSource::Local(directory.join("g.schema.json"))));
        assert_eq!(validation.report(), &Some(directory.join("report.json")));
    }

    #[test]
    fn a_manifest_read_from_a_document_keeps_its_relative_paths_as_written() {
        let manifest = Manifest::from_json5(RELATIVE_PATHS, Path::new("run/run.json5")).unwrap();

        assert_eq!(manifest.inputs().iter().next().unwrap().source(), &Input::Local(PathBuf::from("data/a.csv")));
    }

    #[test]
    fn a_manifest_round_trips_through_its_json5_rendering() {
        let manifest = Manifest::from_json5(MINIMAL, Path::new("manifest.json5")).unwrap();
        let rendered = manifest.to_json5().unwrap();

        assert_eq!(Manifest::from_json5(&rendered, Path::new("manifest.json5")).unwrap(), manifest);
    }

    /// A manifest whose credential fields read `CARGO_PKG_NAME`, which cargo sets for every test run,
    /// so the test needs no environment of its own.
    const READS_ENVIRONMENT: &str = r#"{
        version: "v1",
        vars: { kept: "${CARGO_PKG_NAME}" },
        inputs: [
            {
                source: "https://api.example/records?apikey=${CARGO_PKG_NAME}",
                mapping: "record.json5",
                headers: { "X-Api-Key": "${CARGO_PKG_NAME}" },
            },
        ],
        output: {
            target: "context-broker",
            url: "https://broker.example/${CARGO_PKG_NAME}/",
            headers: { Authorization: "Bearer ${CARGO_PKG_NAME}" },
        },
    }"#;

    /// The value cargo gives `CARGO_PKG_NAME` while this crate's tests run.
    const PACKAGE: &str = "cassiopeia-manifest";

    fn broker(manifest: &Manifest) -> (&MaskedUrl, &RequestHeaders) {
        match manifest.output().as_ref().map(ManifestOutput::destination) {
            Some(Destination::ContextBroker { url, headers, .. }) => (url, headers),
            Some(Destination::File { .. }) | None => panic!("expected a broker destination"),
        }
    }

    #[test]
    fn a_manifest_reads_environment_variables_into_its_credential_fields() {
        let manifest = Manifest::from_json5(READS_ENVIRONMENT, Path::new("manifest.json5")).unwrap();
        let input = manifest.inputs().iter().next().unwrap();
        let Input::Remote(source) = input.source() else {
            panic!("expected a remote source");
        };
        let (url, headers) = broker(&manifest);

        assert_eq!(source.url().as_str(), format!("https://api.example/records?apikey={PACKAGE}"));
        assert_eq!(input.headers().iter().next().unwrap().value().to_str().unwrap(), PACKAGE);
        assert_eq!(url.url().as_str(), format!("https://broker.example/{PACKAGE}/"));
        assert_eq!(headers.iter().next().unwrap().value().to_str().unwrap(), format!("Bearer {PACKAGE}"));
    }

    #[test]
    fn a_value_read_from_the_environment_shows_only_masked() {
        let manifest = Manifest::from_json5(READS_ENVIRONMENT, Path::new("manifest.json5")).unwrap();
        let input = manifest.inputs().iter().next().unwrap();

        assert_eq!(input.source().to_string(), "https://api.example/records?apikey=${CARGO_PKG_NAME}");
        assert_eq!(broker(&manifest).0.to_string(), "https://broker.example/${CARGO_PKG_NAME}/");
        assert!(!format!("{manifest:?}").contains(PACKAGE));
        assert_eq!(
            mask(&format!("GET {}", source_url(&manifest))),
            "GET https://api.example/records?apikey=${CARGO_PKG_NAME}"
        );
    }

    fn source_url(manifest: &Manifest) -> String {
        match manifest.inputs().iter().next().unwrap().source() {
            Input::Remote(url) => url.url().to_string(),
            Input::Local(_) => panic!("expected a remote source"),
        }
    }

    #[test]
    fn a_manifest_that_read_the_environment_renders_its_templates_not_its_values() {
        let manifest = Manifest::from_json5(READS_ENVIRONMENT, Path::new("manifest.json5")).unwrap();
        let rendered = manifest.to_json5().unwrap();

        assert!(!rendered.contains(PACKAGE), "{rendered}");
        assert_eq!(Manifest::from_json5(&rendered, Path::new("manifest.json5")).unwrap(), manifest);
    }

    #[test]
    fn vars_and_other_fields_never_read_the_environment() {
        let manifest = Manifest::from_json5(READS_ENVIRONMENT, Path::new("manifest.json5")).unwrap();

        assert_eq!(manifest.vars().as_ref().unwrap().get("kept"), Some(&serde_json::json!("${CARGO_PKG_NAME}")));
    }

    #[test]
    fn an_unset_variable_fails_naming_the_variable_and_the_field() {
        let error = Manifest::from_json5(
            r#"{ version: "v1", inputs: [{ source: "https://api.example/?k=${CASSIOPEIA_TEST_UNSET_VARIABLE}", mapping: "m.json5" }] }"#,
            Path::new("manifest.json5"),
        )
        .unwrap_err();
        let ManifestError::Parse { source, .. } = &error else {
            panic!("expected a parse error, got {error}");
        };

        assert!(
            source
                .to_string()
                .contains("an input's 'source': the environment variable 'CASSIOPEIA_TEST_UNSET_VARIABLE' is not set"),
            "{source}"
        );
    }

    #[test]
    fn an_unset_output_header_variable_fails_naming_the_header() {
        let error = Manifest::from_json5(
            r#"{ version: "v1", inputs: [{ source: "a.csv", mapping: "m.json5" }],
                 output: { target: "context-broker", url: "https://b/", headers: { Authorization: "Bearer ${CASSIOPEIA_TEST_UNSET_TOKEN}" } } }"#,
            Path::new("manifest.json5"),
        )
        .unwrap_err();
        let ManifestError::Parse { source, .. } = &error else {
            panic!("expected a parse error, got {error}");
        };

        assert!(source.to_string().contains("the output header 'authorization'"), "{source}");
        assert!(source.to_string().contains("CASSIOPEIA_TEST_UNSET_TOKEN"), "{source}");
    }

    #[test]
    fn an_escaped_opening_reads_as_a_literal() {
        let manifest = Manifest::from_json5(
            r#"{ version: "v1", inputs: [{ source: "https://api.example/?q=$${CARGO_PKG_NAME}", mapping: "m.json5" }] }"#,
            Path::new("manifest.json5"),
        )
        .unwrap();

        assert_eq!(source_url(&manifest), "https://api.example/?q=${CARGO_PKG_NAME}");
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
