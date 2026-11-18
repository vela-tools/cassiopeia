use crate::{
    error::PipelineError,
    observer::{PipelineResult, RunObserver},
    pipeline::{Pipeline, ValidationOverrides},
    pipeline_config::ExtractionParallelism,
    test_engine::{SHUTDOWN, engine_config},
    test_reporter::{RecordedDiagnostic, RecordingReporter},
};
use cassiopeia_common::{context::mode::AtContextMode, store_kind::StoreKind};
use cassiopeia_diagnostic::code::diagnostic_code::DiagnosticCode;
use cassiopeia_manifest::manifest::Manifest;
use cassiopeia_reporter::reporter::Reporter;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
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
    /// An accept-anything schema for each of these entity types, or Smart Data Models paths such as
    /// `dataModel.Environment/AirQualityObserved`, so the only warnings left are the ones under test.
    Permissive(&'a [&'a str]),
    /// The schemas of this folder, laid out as the validator looks them up.
    Folder(&'a Path),
}

/// Where an end-to-end run writes its entities.
pub(crate) enum RunOutput<'a> {
    /// Files in the run's own folder, with these members spliced into the manifest's `output` object.
    Files(&'a str),
    /// The manifest's whole `output` object, as it is written, such as a broker's.
    Declared(&'a str),
}

/// What stops an end-to-end run.
#[derive(Clone, Copy)]
pub(crate) struct RunStop {
    /// The flag the run's stages read, the one a signal handler raises in a real run.
    pub(crate) flag: &'static AtomicBool,
    /// How many cycles end before the harness raises `flag` itself, between two cycles; `None`
    /// leaves raising it to the test.
    pub(crate) after_cycles: Option<usize>,
}

impl RunStop {
    /// A stop nobody raises.
    pub(crate) const fn never() -> RunStop {
        RunStop {
            flag: &SHUTDOWN,
            after_cycles: None,
        }
    }

    /// A stop the test raises through `flag`.
    pub(crate) const fn raised_by(flag: &'static AtomicBool) -> RunStop {
        RunStop { flag, after_cycles: None }
    }

    /// A stop the harness raises through `flag` once `cycles` cycles have ended.
    pub(crate) const fn after_cycles(flag: &'static AtomicBool, cycles: usize) -> RunStop {
        RunStop {
            flag,
            after_cycles: Some(cycles),
        }
    }
}

/// What an end-to-end run of declared inputs runs with besides its inputs.
pub(crate) struct RunSetup<'a> {
    /// The schemas its entities are validated against.
    pub(crate) schemas: &'a RunSchemas<'a>,
    /// Where it writes its entities.
    pub(crate) output: RunOutput<'a>,
    /// The manifest schedule, or `None` for one cycle.
    pub(crate) schedule: Option<&'a str>,
    /// The manifest's `onFailure` policy, written as the manifest writes it, or `None` for the
    /// default.
    pub(crate) on_failure: Option<&'a str>,
    /// The folder it keeps its cursors in, or `None` for one in the run's own folder.
    pub(crate) cursors: Option<&'a Path>,
    /// What stops it.
    pub(crate) stop: RunStop,
}

impl<'a> RunSetup<'a> {
    /// One cycle into files with `output_extra` spliced into the output, validated against
    /// `schemas`, under the default failure policy, with cursors in the run's own folder, never
    /// stopped.
    pub(crate) const fn files(schemas: &'a RunSchemas<'a>, output_extra: &'a str) -> RunSetup<'a> {
        RunSetup {
            schemas,
            output: RunOutput::Files(output_extra),
            schedule: None,
            on_failure: None,
            cursors: None,
            stop: RunStop::never(),
        }
    }
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
    /// Every info and success line the run logged, such as the line that reports a cycle, in the
    /// order it logged them.
    pub(crate) messages: Vec<String>,
    /// The warning total of every cycle the run ran, as their summaries count them.
    pub(crate) warnings: u64,
    /// The error total of every cycle the run ran, as their summaries count them.
    pub(crate) errors: u64,
    /// Every entity the run wrote, file by file in the order of their names, each file's in the
    /// order it lists them.
    pub(crate) entities: Vec<Value>,
    /// Every file the run wrote, by name, as it wrote it.
    pub(crate) files: BTreeMap<PathBuf, String>,
}

impl RunOutcome {
    /// The diagnostics the run reported under `code`, in the order it reported them.
    pub(crate) fn coded(&self, code: DiagnosticCode) -> Vec<&RecordedDiagnostic> {
        self.diagnostics.iter().filter(|diagnostic| diagnostic.code == code).collect()
    }
}

/// An observer that adds up the warnings and errors of every cycle that ends, and raises the run's
/// stop once as many cycles have ended as the stop waits for.
///
/// A cycle ends, and decides whether it completed, before it tells its observer, so a stop raised
/// here lands between that cycle and the next.
struct CycleTotals {
    warnings: AtomicU64,
    errors: AtomicU64,
    ended: AtomicUsize,
    stop: RunStop,
}

impl CycleTotals {
    /// No cycle ended yet, under `stop`.
    const fn new(stop: RunStop) -> CycleTotals {
        CycleTotals {
            warnings: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            ended: AtomicUsize::new(0),
            stop,
        }
    }
}

impl RunObserver for CycleTotals {
    fn on_complete(&self, result: &PipelineResult) {
        self.warnings.fetch_add(result.warnings, Ordering::SeqCst);
        self.errors.fetch_add(result.errors, Ordering::SeqCst);
        let ended = self.ended.fetch_add(1, Ordering::SeqCst) + 1;
        if self.stop.after_cycles == Some(ended) {
            self.stop.flag.store(true, Ordering::SeqCst);
        }
    }
}

/// The folder holding the run's `schemas`, written under `root` when they are permissive ones.
fn schemas_folder(root: &Path, schemas: &RunSchemas<'_>) -> PathBuf {
    match schemas {
        RunSchemas::Permissive(entity_types) => {
            let folder = root.join("schemas");
            for entity_type in *entity_types {
                let schema = folder.join(format!("{entity_type}.json"));
                fs::create_dir_all(schema.parent().unwrap()).unwrap();
                fs::write(schema, r#"{"type": "object"}"#).unwrap();
            }
            fs::create_dir_all(&folder).unwrap();
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

/// Runs a one-shot pipeline over one CSV document bound as `binding`, writing files, on `store`, with
/// `output_extra` spliced into the manifest's `output` object, and collects what it reported and
/// wrote.
///
/// The output attaches no `@context`.
pub(crate) fn run_csv_end_to_end(store: StoreKind, csv: &str, binding: &RunBinding, entity_types: &[&str], output_extra: &str) -> RunOutcome {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    let data = root.join("data-0.csv");
    fs::write(&data, csv).unwrap();

    let (result, outcome) = run_declared(
        store,
        root,
        &[declare_input(root, 0, &data, binding)],
        &RunSetup::files(&RunSchemas::Permissive(entity_types), output_extra),
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
    let declared = format!(r#"{{ source: "{}", mapping: "{}" }}"#, source.display(), mapping.display());
    attempt_declared_end_to_end(store, &declared, schemas, output_extra)
}

/// Runs a one-shot pipeline over the one manifest input `declared`, written as a manifest writes it,
/// writing files, on `store`, validating against `schemas`, with `output_extra` spliced into the
/// manifest's `output` object, and returns how the run ended with what it reported and wrote.
pub(crate) fn attempt_declared_end_to_end(
    store: StoreKind,
    declared: &str,
    schemas: &RunSchemas<'_>,
    output_extra: &str,
) -> (Result<(), PipelineError>, RunOutcome) {
    let directory = TempDir::new().unwrap();
    run_declared(store, directory.path(), &[declared.to_owned()], &RunSetup::files(schemas, output_extra))
}

/// Runs a pipeline over the one manifest input `declared`, written as a manifest writes it, on
/// `store`, as `setup` says, and returns how the run ended with what it reported and wrote.
pub(crate) fn attempt_setup_end_to_end(store: StoreKind, declared: &str, setup: &RunSetup<'_>) -> (Result<(), PipelineError>, RunOutcome) {
    let directory = TempDir::new().unwrap();
    run_declared(store, directory.path(), &[declared.to_owned()], setup)
}

/// Runs a one-shot pipeline over the `declared` manifest inputs, written as a manifest writes them,
/// writing files, on `store`, validating against an accept-anything schema for each of
/// `entity_types`, and returns how the run ended with what it reported and wrote.
pub(crate) fn attempt_inputs_end_to_end(store: StoreKind, declared: &[String], entity_types: &[&str]) -> (Result<(), PipelineError>, RunOutcome) {
    let directory = TempDir::new().unwrap();
    run_declared(store, directory.path(), declared, &RunSetup::files(&RunSchemas::Permissive(entity_types), ""))
}

/// Runs a pipeline over the one manifest input `declared` on the manifest schedule `schedule`, written
/// as a manifest writes it, and otherwise as [`attempt_declared_end_to_end`] does, and returns how the
/// run ended with what it reported and wrote.
pub(crate) fn attempt_scheduled_end_to_end(
    store: StoreKind,
    declared: &str,
    schemas: &RunSchemas<'_>,
    output_extra: &str,
    schedule: &str,
) -> (Result<(), PipelineError>, RunOutcome) {
    let directory = TempDir::new().unwrap();
    let setup = RunSetup {
        schedule: Some(schedule),
        ..RunSetup::files(schemas, output_extra)
    };
    run_declared(store, directory.path(), &[declared.to_owned()], &setup)
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

    run_declared(store, root, &declared, &RunSetup::files(&RunSchemas::Permissive(entity_types), output_extra))
}

/// Runs a pipeline over the `declared` manifest inputs, with its own files under `root`, as `setup`
/// says, and returns how the run ended, or failed to be built, with what it reported and wrote.
fn run_declared(store: StoreKind, root: &Path, declared: &[String], setup: &RunSetup<'_>) -> (Result<(), PipelineError>, RunOutcome) {
    let folders = RunFolders {
        schemas: schemas_folder(root, setup.schemas),
        cursors: setup.cursors.map_or_else(|| root.join("cursors"), Path::to_path_buf),
        output: root.join("out"),
    };

    let schedule = setup.schedule.map(|schedule| format!(", schedule: {schedule}")).unwrap_or_default();
    let on_failure = setup.on_failure.map(|policy| format!(", onFailure: {policy}")).unwrap_or_default();
    let output_object = match setup.output {
        RunOutput::Files(output_extra) => format!(r#"{{ target: "file", directory: "{}"{output_extra} }}"#, folders.output.display()),
        RunOutput::Declared(object) => object.to_owned(),
    };
    let document = format!(
        r#"{{ version: "v1", inputs: [{}], output: {output_object}{schedule}{on_failure} }}"#,
        declared.join(", ")
    );
    let manifest = Manifest::from_json5(&document, Path::new("manifest.json5")).unwrap();

    run_manifest(store, &manifest, folders, ExtractionParallelism::Parallel, setup.stop)
}

/// The folders a run keeps its own files in.
pub(crate) struct RunFolders {
    /// The schemas its entities are validated against.
    pub(crate) schemas: PathBuf,
    /// The folder it keeps its cursors in.
    pub(crate) cursors: PathBuf,
    /// The folder its manifest's output writes entities into, read back once the run ends.
    pub(crate) output: PathBuf,
}

/// Runs a pipeline over `manifest` on `store`, with its own files in `folders`, extracting records
/// as `extraction` says, stopped as `stop` says, and returns how the run ended, or failed to be
/// built, with what it reported and wrote.
///
/// The run attaches no `@context` unless the manifest's output declares one.
pub(crate) fn run_manifest(
    store: StoreKind,
    manifest: &Manifest,
    folders: RunFolders,
    extraction: ExtractionParallelism,
    stop: RunStop,
) -> (Result<(), PipelineError>, RunOutcome) {
    let RunFolders { schemas, cursors, output } = folders;
    let reporter: &'static RecordingReporter = Box::leak(Box::new(RecordingReporter::new()));
    let totals = Arc::new(CycleTotals::new(stop));
    let observer: Arc<dyn RunObserver> = Arc::clone(&totals) as Arc<dyn RunObserver>;
    let mut engine = engine_config();
    engine.entity_store = store;
    engine.relationship_store = store;
    engine.context_mode = AtContextMode::None;
    engine.extraction = extraction;
    engine.schemas_folder = schemas;
    engine.cursors_folder = cursors;
    let built = Pipeline::from_manifest(
        manifest,
        ValidationOverrides {
            report_path: None,
            mode: None,
            schema: None,
            representation: None,
        },
        serde_json::Map::new(),
        engine,
        reporter as &'static dyn Reporter,
        stop.flag,
        observer,
    );
    // The pipeline holds the run's cursor stores until it is dropped, at the end of this block.
    let result = built.and_then(|mut pipeline| pipeline.run());

    let files = written_files(&output);
    let outcome = RunOutcome {
        diagnostics: reporter.diagnostics(),
        messages: reporter.messages(),
        warnings: totals.warnings.load(Ordering::SeqCst),
        errors: totals.errors.load(Ordering::SeqCst),
        entities: files.values().flat_map(|file| file_entities(file)).collect(),
        files,
    };
    (result, outcome)
}

/// Every file written into `output`, by name, or none when the run never created the directory.
fn written_files(output: &Path) -> BTreeMap<PathBuf, String> {
    let files = match fs::read_dir(output) {
        Ok(files) => files,
        Err(error) if error.kind() == ErrorKind::NotFound => return BTreeMap::new(),
        Err(error) => panic!("the output directory cannot be read: {error}"),
    };
    files
        .map(|file| {
            let path = file.unwrap().path();
            (PathBuf::from(path.file_name().unwrap()), fs::read_to_string(&path).unwrap())
        })
        .collect()
}

/// The entities an entity file lists, in its order.
fn file_entities(file: &str) -> Vec<Value> {
    let Value::Array(entities) = serde_json::from_str(file).unwrap() else {
        panic!("an entity file must hold a JSON array");
    };
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
