mod case;
mod divergence;
mod mismatch;
mod record;
mod run;
mod stored_snapshot;
mod view;

use crate::end_to_end::golden::{
    case::GoldenCase,
    mismatch::{entities_mismatch, report_mismatch},
    run::run_view,
    stored_snapshot::print_mismatch,
    view::{REPRESENTATIONS, View},
};
use insta::{Settings, assert_json_snapshot, glob};
use pretty_assertions::assert_eq;
use std::path::Path;

/// The corpus: one directory per group of cases, and one directory per case in each group.
const CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden");

/// Writes every view of `case` and checks each against its snapshot in the case's directory.
///
/// A history's runs differ only in the representation they write, so they must report the same
/// diagnostics; that is checked outright and recorded once per history. Before a snapshot is
/// checked, where it first differs is printed, since insta's own diff need not name the entity or
/// the diagnostic it belongs to.
fn record_case(case: &GoldenCase) {
    for &history in case.histories() {
        let [normalized, concise, simplified] = REPRESENTATIONS.map(|representation| {
            let view = View { history, representation };
            let record = run_view(case, view);
            let name = view.snapshot_name();
            print_mismatch(case, &name, &record.entities, entities_mismatch);
            assert_json_snapshot!(name, record.entities);
            record.report
        });
        assert_eq!(normalized, concise, "{}: {history} reports differ by representation", case.name().display());
        assert_eq!(normalized, simplified, "{}: {history} reports differ by representation", case.name().display());
        let name = format!("{history}-diagnostics");
        print_mismatch(case, &name, &normalized, report_mismatch);
        assert_json_snapshot!(name, normalized);
    }
}

#[test]
fn every_golden_case_writes_and_reports_what_its_snapshots_record() {
    let corpus = Path::new(CORPUS).canonicalize().unwrap();
    glob!(&corpus, "*/*/manifest.json5", |manifest| {
        let case = GoldenCase::read(&corpus, manifest);
        let mut settings = Settings::clone_current();
        settings.set_snapshot_path(case.directory());
        settings.remove_snapshot_suffix();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_omit_expression(true);
        settings.bind(|| record_case(&case));
    });
}
