use crate::{
    end_to_end::{
        golden::{
            case::GoldenCase,
            record::{Placeholders, ViewRecord},
            view::View,
        },
        harness::{RunFolders, run_manifest},
    },
    pipeline_config::ExtractionParallelism,
    test_engine::SHUTDOWN,
};
use cassiopeia_common::store_kind::StoreKind;
use pretty_assertions::assert_eq;
use std::fs;
use temp_dir::TempDir;

/// Runs `case` in `view` on both entity stores and returns what the Redb run wrote and reported.
///
/// A diagnostic that stands for several occurrences names the first one it met. Records are
/// extracted on one thread, and either store hands entities on in the order of their first records,
/// so a run writes its entities and names its examples the same on every run and store. The two runs
/// are held to the same entities in the same order and to the same diagnostics. A difference there is
/// a defect of a store rather than a change to what the mapping writes, and fails the corpus
/// outright.
pub(crate) fn run_view(case: &GoldenCase, view: View) -> ViewRecord {
    let in_memory = run_on_store(case, view, StoreKind::DashMap);
    let on_disk = run_on_store(case, view, StoreKind::Redb);
    let directory = case.directory().display();
    assert_eq!(
        in_memory.entities, on_disk.entities,
        "{directory} in {view:?}: the stores wrote different entities"
    );
    assert_eq!(
        in_memory.report, on_disk.report,
        "{directory} in {view:?}: the stores reported different diagnostics"
    );
    on_disk
}

/// Runs `case` in `view` on `store`, in a temporary folder of its own, and records what it wrote and
/// reported.
fn run_on_store(case: &GoldenCase, view: View, store: StoreKind) -> ViewRecord {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    let folders = RunFolders {
        schemas: root.join("schemas"),
        cursors: root.join("cursors"),
        output: root.join("out"),
    };
    fs::create_dir_all(&folders.schemas).unwrap();
    let manifest = case.manifest(view, &folders.output);
    let (result, outcome) = run_manifest(store, &manifest, folders, ExtractionParallelism::Sequential, &SHUTDOWN);
    let placeholders = Placeholders {
        case: case.directory(),
        run: root,
    };
    ViewRecord::new(result, outcome, &placeholders)
}
