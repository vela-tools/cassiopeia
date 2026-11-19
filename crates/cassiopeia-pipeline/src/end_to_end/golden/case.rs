use crate::end_to_end::golden::view::{History, View};
use cassiopeia_manifest::{manifest::Manifest, mapping_binding::MappingBinding};
use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
use serde::Serialize;
use serde_json::{Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The histories of a case whose mappings declare no `observedAt`.
const CURRENT_ONLY: &[History] = &[History::Current];

/// The histories of a case whose mappings declare an `observedAt`.
const CURRENT_AND_SERIES: &[History] = &[History::Current, History::Series];

/// One case of the golden corpus: a directory holding a manifest that declares no `output`, the
/// mappings and inputs it names by relative path, and the snapshots of what it writes and reports.
pub(crate) struct GoldenCase {
    /// The case's directory relative to the corpus, such as `values/json-property`, which names it.
    name: PathBuf,
    /// The case's directory, which its relative paths resolve against and its snapshots live in.
    directory: PathBuf,
    /// Its manifest file, named when the manifest fails to parse.
    origin: PathBuf,
    /// Its manifest document, which each run adds its own `output` to.
    document: Map<String, Value>,
    /// The histories it is written in.
    histories: &'static [History],
}

/// A case's manifest document with the `output` of one run added.
#[derive(Serialize)]
struct RunDocument<'a> {
    #[serde(flatten)]
    case: &'a Map<String, Value>,
    output: Value,
}

impl GoldenCase {
    /// Reads the case whose manifest is at `manifest`, in the corpus at `corpus`.
    pub(crate) fn read(corpus: &Path, manifest: &Path) -> GoldenCase {
        let content = fs::read_to_string(manifest).unwrap();
        let Value::Object(document) = serde_json5::from_str(&content).unwrap() else {
            panic!("{} must hold a JSON5 object", manifest.display());
        };
        assert!(
            !document.contains_key("output"),
            "{} must leave `output` to the corpus, which writes every view",
            manifest.display()
        );
        let directory = manifest.parent().unwrap().to_path_buf();
        let name = directory.strip_prefix(corpus).unwrap().to_path_buf();
        let histories = if mappings_declare_observed_at(manifest) {
            CURRENT_AND_SERIES
        } else {
            CURRENT_ONLY
        };

        GoldenCase {
            name,
            directory,
            origin: manifest.to_path_buf(),
            document,
            histories,
        }
    }

    /// The case's directory relative to the corpus, which names it in every message.
    pub(crate) fn name(&self) -> &Path {
        &self.name
    }

    /// The case's directory.
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    /// The histories the case is written in: the current state, and a series when any of its
    /// mappings declares an `observedAt`.
    pub(crate) const fn histories(&self) -> &'static [History] {
        self.histories
    }

    /// The case's manifest, writing `view` as files into `directory`.
    pub(crate) fn manifest(&self, view: View, directory: &Path) -> Manifest {
        let document = RunDocument {
            case: &self.document,
            output: view.output(directory),
        };
        let content = serde_json::to_string(&document).unwrap();
        Manifest::from_json5(&content, &self.origin).unwrap().resolved_against(&self.directory)
    }
}

/// Whether any mapping the manifest at `manifest` binds loads and declares an `observedAt`.
///
/// A mapping that does not load writes nothing in any history, so it asks for no series.
fn mappings_declare_observed_at(manifest: &Path) -> bool {
    let manifest = Manifest::from_file(manifest).unwrap();
    let mut runner = TemplateRunner::new();
    manifest
        .inputs()
        .iter()
        .flat_map(|input| match input.mapping_binding() {
            MappingBinding::Single { mapping } => vec![mapping.as_path()],
            MappingBinding::Collections { mappings } => mappings.iter().map(|collection| collection.mapping().as_path()).collect(),
        })
        .any(|mapping| Mapping::from_file(mapping, &mut runner).is_ok_and(|mapping| mapping.is_temporal()))
}
