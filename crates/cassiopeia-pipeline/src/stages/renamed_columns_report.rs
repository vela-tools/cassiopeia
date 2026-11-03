use cassiopeia_csv_column::renamed_column::RenamedColumn;
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::path::Path;

/// How many renames the headline names before it counts the rest, so a header that repeats a name
/// dozens of times still fits on one line.
const HEADLINE_RENAMES: usize = 3;

/// The one-line summary: the file, and the first [`HEADLINE_RENAMES`] renamed columns with the key
/// each is read by, the rest counted.
///
/// The renames ride on the headline because the concise form shows nothing else, and a mapping
/// author needs the generated key to read the column at all. Every rename is also attached as its
/// own field, which the verbose form lists in full.
fn headline(path: &Path, renamed: &[RenamedColumn]) -> String {
    let columns = match renamed.len() {
        1 => "1 column was".to_owned(),
        count => format!("{count} columns were"),
    };
    let named = renamed
        .iter()
        .take(HEADLINE_RENAMES)
        .map(RenamedColumn::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    let details = match renamed.len().saturating_sub(HEADLINE_RENAMES) {
        0 => named,
        rest => format!("{named}; and {rest} more"),
    };
    format!(
        "The header of `{}` gives more than one column the same record key, so {columns} renamed: {details}",
        path.display()
    )
}

/// Names a CSV file's renamed columns in one warning and returns how many warnings that was: one
/// when any column was renamed, none otherwise.
///
/// One diagnostic per file however many columns were renamed, since the renames share one cause: the
/// file's header.
pub(crate) fn report_renamed_columns(sink: &dyn DiagnosticSink, path: &Path, renamed: &[RenamedColumn]) -> u64 {
    if renamed.is_empty() {
        return 0;
    }

    let builder = renamed.iter().fold(
        DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Ingest(IngestCode::ColumnsRenamed), headline(path, renamed))
            .with_context(ContextField::SourcePath(path.to_path_buf())),
        // The diagnostic outlives the ingestor's column keys, so each rename is copied into it.
        |builder, column| builder.with_context(ContextField::RenamedColumn(column.clone())),
    );
    sink.report(&builder.build());
    1
}

#[cfg(test)]
mod tests {
    use crate::{stages::renamed_columns_report::report_renamed_columns, test_reporter::RecordingReporter};
    use cassiopeia_csv_column::{column_key::ColumnKey, column_number::ColumnNumber, renamed_column::RenamedColumn};
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
        context_field::ContextField,
    };
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};

    fn renamed(wanted: &str, holder: usize, column: usize, key: &str) -> RenamedColumn {
        RenamedColumn {
            wanted: ColumnKey::from(wanted.to_owned()),
            holder: ColumnNumber::of_index(holder),
            column: ColumnNumber::of_index(column),
            key: ColumnKey::from(key.to_owned()),
        }
    }

    #[test]
    fn a_file_without_renamed_columns_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_renamed_columns(&sink, Path::new("/data/plain.csv"), &[]), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_renamed_column_is_one_warning_naming_the_file_both_columns_and_both_keys() {
        let sink = RecordingReporter::new();

        let total = report_renamed_columns(&sink, Path::new("/data/values.csv"), &[renamed("value", 1, 2, "value@3")]);

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::ColumnsRenamed));
        assert_eq!(reported[0].code.to_string(), "ingest-columns-renamed");
        assert_eq!(
            reported[0].headline,
            "The header of `/data/values.csv` gives more than one column the same record key, so 1 column was renamed: column 3 `value` is read as `value@3`, as column 2 keeps `value`"
        );
        assert_eq!(reported[0].occurrences, 1);
        assert_eq!(reported[0].incidental, [ContextField::SourcePath(PathBuf::from("/data/values.csv"))]);
        assert_eq!(reported[0].defining, [ContextField::RenamedColumn(renamed("value", 1, 2, "value@3"))]);
    }

    #[test]
    fn several_renamed_columns_of_one_file_are_still_one_warning() {
        let sink = RecordingReporter::new();

        let total = report_renamed_columns(
            &sink,
            Path::new("/data/values.csv"),
            &[renamed("value", 1, 2, "value@3"), renamed("2", 0, 3, "2@4")],
        );

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(
            reported[0].headline,
            "The header of `/data/values.csv` gives more than one column the same record key, so 2 columns were renamed: column 3 `value` is read as `value@3`, as column 2 keeps `value`; column 4 `2` is read as `2@4`, as column 1 keeps `2`"
        );
        assert_eq!(
            reported[0].defining,
            [
                ContextField::RenamedColumn(renamed("value", 1, 2, "value@3")),
                ContextField::RenamedColumn(renamed("2", 0, 3, "2@4")),
            ]
        );
    }

    #[test]
    fn a_headline_names_three_renames_and_counts_the_rest_while_every_rename_stays_attached() {
        let sink = RecordingReporter::new();
        let columns: Vec<RenamedColumn> = (1..=5).map(|index| renamed("Unit", 0, index, &format!("Unit@{}", index + 1))).collect();

        report_renamed_columns(&sink, Path::new("/data/units.csv"), &columns);

        let reported = sink.diagnostics();
        assert_eq!(
            reported[0].headline,
            "The header of `/data/units.csv` gives more than one column the same record key, so 5 columns were renamed: column 2 `Unit` is read as `Unit@2`, as column 1 keeps `Unit`; column 3 `Unit` is read as `Unit@3`, as column 1 keeps `Unit`; column 4 `Unit` is read as `Unit@4`, as column 1 keeps `Unit`; and 2 more"
        );
        assert_eq!(reported[0].defining, columns.into_iter().map(ContextField::RenamedColumn).collect::<Vec<_>>());
    }

    #[test]
    fn exactly_three_renames_fit_the_headline_without_a_count() {
        let sink = RecordingReporter::new();
        let columns: Vec<RenamedColumn> = (1..=3).map(|index| renamed("Unit", 0, index, &format!("Unit@{}", index + 1))).collect();

        report_renamed_columns(&sink, Path::new("/data/units.csv"), &columns);

        assert_eq!(
            sink.diagnostics()[0].headline,
            "The header of `/data/units.csv` gives more than one column the same record key, so 3 columns were renamed: column 2 `Unit` is read as `Unit@2`, as column 1 keeps `Unit`; column 3 `Unit` is read as `Unit@3`, as column 1 keeps `Unit`; column 4 `Unit` is read as `Unit@4`, as column 1 keeps `Unit`"
        );
    }
}
