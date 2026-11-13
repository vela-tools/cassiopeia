use crate::{
    error::PipelineError,
    observer::{PipelineResult, RunObserver},
    pipeline::{Pipeline, ValidationOverrides},
    test_engine::{SHUTDOWN, engine_config},
    test_reporter::{RecordedDiagnostic, RecordingReporter},
};
use cassiopeia_common::{context::mode::AtContextMode, store_kind::StoreKind};
use cassiopeia_diagnostic::code::diagnostic_code::DiagnosticCode;
use cassiopeia_manifest::manifest::Manifest;
use cassiopeia_reporter::reporter::Reporter;
use serde_json::Value;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};
use temp_dir::TempDir;

/// How one end-to-end input binds its records to mapping documents.
pub(crate) enum RunBinding {
    /// One mapping document for every record, declared as `mapping`.
    Mapping(&'static str),
    /// One mapping document per collection label, declared as `mappings`.
    Collections(&'static [(&'static str, &'static str)]),
}

/// The schemas an end-to-end run validates its entities against.
pub(crate) enum RunSchemas<'a> {
    /// An accept-anything schema for each of these entity types, so the only warnings left are the
    /// ones under test.
    Permissive(&'a [&'a str]),
    /// The schemas of this folder, laid out as the validator looks them up.
    Folder(&'a Path),
}

/// One input of an end-to-end run: how it binds mappings and the document it reads, whose format
/// detection decides, so a `GeoJSON` document reaches the `GeoJSON` ingestor and any other the JSON one.
pub(crate) struct RunInput {
    pub(crate) binding: RunBinding,
    pub(crate) records: Value,
}

/// What one end-to-end run left behind.
pub(crate) struct RunOutcome {
    /// Every diagnostic the run reported, in the order it reported them.
    pub(crate) diagnostics: Vec<RecordedDiagnostic>,
    /// The warning total of the run's one completed cycle.
    pub(crate) warnings: u64,
    /// Every entity the run wrote, across all of its output files.
    pub(crate) entities: Vec<Value>,
}

impl RunOutcome {
    /// The diagnostics the run reported under `code`, in the order it reported them.
    pub(crate) fn coded(&self, code: DiagnosticCode) -> Vec<&RecordedDiagnostic> {
        self.diagnostics.iter().filter(|diagnostic| diagnostic.code == code).collect()
    }
}

/// An observer that keeps the warning total of every completed cycle.
#[derive(Default)]
struct WarningTotals(Mutex<Vec<u64>>);

impl RunObserver for WarningTotals {
    fn on_complete(&self, result: &PipelineResult) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(result.warnings);
    }
}

/// The folder holding the run's `schemas`, written under `root` when they are permissive ones.
fn schemas_folder(root: &Path, schemas: &RunSchemas<'_>) -> PathBuf {
    match schemas {
        RunSchemas::Permissive(entity_types) => {
            let folder = root.join("schemas");
            fs::create_dir_all(&folder).unwrap();
            for entity_type in *entity_types {
                fs::write(folder.join(format!("{entity_type}.json")), r#"{"type": "object"}"#).unwrap();
            }
            folder
        }
        RunSchemas::Folder(folder) => folder.to_path_buf(),
    }
}

/// Runs a one-shot pipeline over `inputs`, writing files, on `store`, with `output_extra` spliced
/// into the manifest's `output` object, and collects what it reported and wrote.
///
/// The output attaches no `@context` unless `output_extra` declares one.
pub(crate) fn run_end_to_end(store: StoreKind, inputs: &[RunInput], entity_types: &[&str], output_extra: &str) -> RunOutcome {
    let (result, outcome) = attempt_end_to_end(store, inputs, entity_types, output_extra);
    result.unwrap();
    outcome
}

/// Runs a one-shot pipeline like [`run_end_to_end`] that must fail, and returns the failure with
/// what the run reported and wrote before it.
pub(crate) fn run_failing_end_to_end(store: StoreKind, inputs: &[RunInput], entity_types: &[&str], output_extra: &str) -> (PipelineError, RunOutcome) {
    let (result, outcome) = attempt_end_to_end(store, inputs, entity_types, output_extra);
    let Err(error) = result else {
        panic!("the run was expected to fail");
    };
    (error, outcome)
}

/// Runs a one-shot pipeline over one CSV document bound as `binding`, writing files, on `store`, and
/// collects what it reported and wrote.
///
/// The output attaches no `@context`.
pub(crate) fn run_csv_end_to_end(store: StoreKind, csv: &str, binding: &RunBinding, entity_types: &[&str]) -> RunOutcome {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    let data = root.join("data-0.csv");
    fs::write(&data, csv).unwrap();

    let (result, outcome) = run_declared(
        store,
        root,
        &[declare_input(root, 0, &data, binding)],
        &RunSchemas::Permissive(entity_types),
        "",
    );
    result.unwrap();
    outcome
}

/// Runs a one-shot pipeline over the local file `source`, mapped by the mapping document at
/// `mapping`, writing files, on `store`, validating against `schemas`, with `output_extra` spliced
/// into the manifest's `output` object, and returns how the run ended with what it reported and
/// wrote.
///
/// The input declares no format, so the file's format is detected.
pub(crate) fn attempt_file_end_to_end(
    store: StoreKind,
    source: &Path,
    mapping: &Path,
    schemas: &RunSchemas<'_>,
    output_extra: &str,
) -> (Result<(), PipelineError>, RunOutcome) {
    let directory = TempDir::new().unwrap();
    let declared = format!(r#"{{ source: "{}", mapping: "{}" }}"#, source.display(), mapping.display());
    run_declared(store, directory.path(), &[declared], schemas, output_extra)
}

/// Runs a one-shot pipeline over `inputs` and returns how the run ended with what it reported and
/// wrote.
fn attempt_end_to_end(store: StoreKind, inputs: &[RunInput], entity_types: &[&str], output_extra: &str) -> (Result<(), PipelineError>, RunOutcome) {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    let declared: Vec<String> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let data = root.join(format!("data-{index}.json"));
            fs::write(&data, input.records.to_string()).unwrap();
            declare_input(root, index, &data, &input.binding)
        })
        .collect();

    run_declared(store, root, &declared, &RunSchemas::Permissive(entity_types), output_extra)
}

/// Runs a one-shot pipeline over the `declared` manifest inputs, with its schemas and output under
/// `root`, and returns how the run ended with what it reported and wrote.
fn run_declared(store: StoreKind, root: &Path, declared: &[String], schemas: &RunSchemas<'_>, output_extra: &str) -> (Result<(), PipelineError>, RunOutcome) {
    let schemas_folder = schemas_folder(root, schemas);
    let output = root.join("out");

    let document = format!(
        r#"{{ version: "v1", inputs: [{}], output: {{ target: "file", directory: "{}"{output_extra} }} }}"#,
        declared.join(", "),
        output.display()
    );
    let manifest = Manifest::from_json5(&document, Path::new("manifest.json5")).unwrap();

    let reporter: &'static RecordingReporter = Box::leak(Box::new(RecordingReporter::new()));
    let totals = Arc::new(WarningTotals::default());
    let observer: Arc<dyn RunObserver> = Arc::clone(&totals) as Arc<dyn RunObserver>;
    let mut engine = engine_config();
    engine.entity_store = store;
    engine.relationship_store = store;
    engine.context_mode = AtContextMode::None;
    engine.schemas_folder = schemas_folder;
    let mut pipeline = Pipeline::from_manifest(
        &manifest,
        ValidationOverrides {
            report_path: None,
            mode: None,
            schema: None,
            representation: None,
        },
        serde_json::Map::new(),
        engine,
        reporter as &'static dyn Reporter,
        &SHUTDOWN,
        observer,
    )
    .unwrap();
    let result = pipeline.run();

    let warnings = totals.0.lock().unwrap_or_else(PoisonError::into_inner).iter().sum();
    let outcome = RunOutcome {
        diagnostics: reporter.diagnostics(),
        warnings,
        entities: written_entities(&output),
    };
    (result, outcome)
}

/// Every entity written into `output`, across all of its files, or none when the run never
/// created the directory.
fn written_entities(output: &Path) -> Vec<Value> {
    let files = match fs::read_dir(output) {
        Ok(files) => files,
        Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(error) => panic!("the output directory cannot be read: {error}"),
    };
    let mut entities = Vec::new();
    for file in files {
        let written: Value = serde_json::from_str(&fs::read_to_string(file.unwrap().path()).unwrap()).unwrap();
        let Value::Array(written) = written else {
            panic!("an entity file must hold a JSON array");
        };
        entities.extend(written);
    }
    entities
}

/// Declares the input at `index`, reading `data`, as a manifest input, writing its mapping documents
/// into `root`.
fn declare_input(root: &Path, index: usize, data: &Path, binding: &RunBinding) -> String {
    let binding = declare_binding(root, index, binding);
    format!(r#"{{ source: "{}", {binding} }}"#, data.display())
}

/// Writes the mapping documents of the input at `index` into `root` and declares them as the input's
/// `mapping` or `mappings` member.
fn declare_binding(root: &Path, index: usize, binding: &RunBinding) -> String {
    match binding {
        RunBinding::Mapping(document) => {
            let mapping = root.join(format!("mapping-{index}.json5"));
            fs::write(&mapping, document).unwrap();
            format!(r#"mapping: "{}""#, mapping.display())
        }
        RunBinding::Collections(collections) => {
            let entries: Vec<String> = collections
                .iter()
                .enumerate()
                .map(|(position, (collection, document))| {
                    let mapping = root.join(format!("mapping-{index}-{position}.json5"));
                    fs::write(&mapping, document).unwrap();
                    format!(r#"{{ collection: "{collection}", mapping: "{}" }}"#, mapping.display())
                })
                .collect();
            format!("mappings: [{}]", entries.join(", "))
        }
    }
}

/// The ids of every written entity, sorted.
pub(crate) fn entity_ids(outcome: &RunOutcome) -> Vec<String> {
    let mut ids: Vec<String> = outcome
        .entities
        .iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();
    ids.sort();
    ids
}

/// Both entity-store backends a run can select.
pub(crate) const STORES: [StoreKind; 2] = [StoreKind::DashMap, StoreKind::Redb];
