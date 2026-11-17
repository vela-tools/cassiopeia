mod case;
mod record;
mod run;
mod view;

use crate::end_to_end::golden::{
    case::GoldenCase,
    run::run_view,
    view::{REPRESENTATIONS, View},
};
use insta::{Settings, assert_json_snapshot, glob};
use pretty_assertions::assert_eq;

/// Writes every view of `case` and checks each against its snapshot in the case's directory.
///
/// A history's runs differ only in the representation they write, so they must report the same
/// diagnostics; that is checked outright and recorded once per history.
fn record_case(case: &GoldenCase) {
    for &history in case.histories() {
        let [normalized, concise, simplified] = REPRESENTATIONS.map(|representation| {
            let view = View { history, representation };
            let record = run_view(case, view);
            assert_json_snapshot!(view.snapshot_name(), record.entities);
            record.report
        });
        assert_eq!(
            normalized,
            concise,
            "{}: {history} reports differ by representation",
            case.directory().display()
        );
        assert_eq!(
            normalized,
            simplified,
            "{}: {history} reports differ by representation",
            case.directory().display()
        );
        assert_json_snapshot!(format!("{history}-diagnostics"), normalized);
    }
}

#[test]
fn every_golden_case_writes_and_reports_what_its_snapshots_record() {
    glob!("../../../golden", "*/*/manifest.json5", |manifest| {
        let case = GoldenCase::read(manifest);
        let mut settings = Settings::clone_current();
        settings.set_snapshot_path(case.directory());
        settings.remove_snapshot_suffix();
        settings.set_prepend_module_to_snapshot(false);
        settings.set_omit_expression(true);
        settings.bind(|| record_case(&case));
    });
}
