use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ingestor::csv::renamed_column::RenamedColumn;
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::path::Path;

/// The one-line summary: the file, and for every renamed column its number, the key its header asks
/// for, the key it is read by instead, and the column that keeps the asked-for key.
///
/// Every rename rides on the headline because the concise form shows nothing else, and a mapping
/// author needs the generated key to read the column at all.
fn headline(path: &Path, renamed: &[RenamedColumn]) -> String {
    let columns = match renamed.len() {
        1 => "1 column was".to_owned(),
        count => format!("{count} columns were"),
    };
    let details = renamed
        .iter()
        .map(|column| {
            format!(
                "column {} `{}` is read as `{}`, as column {} keeps `{}`",
                column.column, column.wanted, column.key, column.holder, column.wanted
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "the header of `{}` gives more than one column the same record key, so {columns} renamed: {details}",
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

    sink.report(
        &DiagnosticBuilder::new(Severity::Warning, DiagnosticCode::Ingest(IngestCode::ColumnsRenamed), headline(path, renamed))
            .with_context(ContextField::SourcePath(path.to_path_buf()))
            .build(),
    );
    1
}

#[cfg(test)]
mod tests {
    use crate::{stages::renamed_columns_report::report_renamed_columns, test_reporter::RecordingReporter};
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
        context_field::ContextField,
    };
    use cassiopeia_ingestor::csv::{column_key::ColumnKey, column_number::ColumnNumber, renamed_column::RenamedColumn};
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
            "the header of `/data/values.csv` gives more than one column the same record key, so 1 column was renamed: column 3 `value` is read as `value@3`, as column 2 keeps `value`"
        );
        assert_eq!(reported[0].occurrences, 1);
        assert_eq!(reported[0].incidental, [ContextField::SourcePath(PathBuf::from("/data/values.csv"))]);
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
            "the header of `/data/values.csv` gives more than one column the same record key, so 2 columns were renamed: column 3 `value` is read as `value@3`, as column 2 keeps `value`; column 4 `2` is read as `2@4`, as column 1 keeps `2`"
        );
    }
}
