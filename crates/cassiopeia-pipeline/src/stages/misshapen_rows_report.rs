use cassiopeia_common::input::Input;
use cassiopeia_csv_dialect::misshapen_row::MisshapenRow;
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ingestor::csv::misshapen_rows::MisshapenRows;
use cassiopeia_reporter::reporter::DiagnosticSink;
use std::num::NonZeroU64;

/// `count` fields, in words.
fn fields(count: usize) -> String {
    match count {
        1 => "1 field".to_owned(),
        count => format!("{count} fields"),
    }
}

/// One named row: where it sits and how many fields it has.
fn named(row: &MisshapenRow) -> String {
    format!("row {} has {}", row.row, fields(row.fields))
}

/// The one-line summary: the source, the table's field count, the named rows, and the rest counted.
///
/// The rows ride on the headline because the concise form shows nothing else, and a row number is
/// what lets a reader find the row in the file.
fn headline(source: &Input, table_fields: usize, misshapen: &MisshapenRows, total: NonZeroU64) -> String {
    let rows = match total.get() {
        1 => "1 row".to_owned(),
        count => format!("{count} rows"),
    };
    let skipped = match total.get() {
        1 => "it was skipped",
        _ => "they were skipped",
    };
    let listed = misshapen.named().iter().map(named).collect::<Vec<_>>().join("; ");
    let details = match misshapen.unnamed() {
        0 => listed,
        rest => format!("{listed}; and {rest} more"),
    };
    format!(
        "The table in `{source}` has {} per row, but {rows} had another number, so {skipped}: {details}",
        fields(table_fields)
    )
}

/// Names a CSV file's skipped rows in one warning and returns how many rows that was.
///
/// One diagnostic per file, carrying every skipped row as an occurrence, since the rows share one
/// cause: they do not fit the file's table. The file is named by its `source`, so a downloaded one
/// is named by its URL, the same in every run, rather than by the temporary copy it was read from.
pub(crate) fn report_misshapen_rows(sink: &dyn DiagnosticSink, source: &Input, rows: &MisshapenRows) -> u64 {
    let (Some(total), Some(first)) = (NonZeroU64::new(rows.total()), rows.named().first()) else {
        return 0;
    };

    sink.report(
        &DiagnosticBuilder::new(
            Severity::Warning,
            DiagnosticCode::Ingest(IngestCode::RowsMisshapen),
            headline(source, first.table_fields, rows, total),
        )
        // The diagnostic outlives the ingestor that names the source.
        .with_context(ContextField::Source(source.clone()))
        .with_occurrences(total)
        .build(),
    );
    total.get()
}

#[cfg(test)]
mod tests {
    use crate::{stages::misshapen_rows_report::report_misshapen_rows, test_reporter::RecordingReporter};
    use cassiopeia_common::input::Input;
    use cassiopeia_csv_dialect::{misshapen_row::MisshapenRow, row_number::RowNumber};
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
        context_field::ContextField,
    };
    use cassiopeia_ingestor::csv::misshapen_rows::MisshapenRows;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;
    use url::Url;

    fn rows(skipped: &[(u64, usize)]) -> MisshapenRows {
        let mut rows = MisshapenRows::default();
        for &(above, fields) in skipped {
            rows.record(MisshapenRow {
                row: RowNumber::after(above),
                fields,
                table_fields: 3,
            });
        }
        rows
    }

    #[test]
    fn a_file_without_skipped_rows_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_misshapen_rows(&sink, &Input::Local(PathBuf::from("/data/values.csv")), &rows(&[])), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn one_skipped_row_is_one_warning_naming_the_file_the_row_and_both_widths() {
        let sink = RecordingReporter::new();

        let total = report_misshapen_rows(&sink, &Input::Local(PathBuf::from("/data/values.csv")), &rows(&[(5, 1)]));

        let reported = sink.diagnostics();
        assert_eq!(total, 1);
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::RowsMisshapen));
        assert_eq!(reported[0].code.to_string(), "ingest-rows-misshapen");
        assert_eq!(
            reported[0].headline,
            "The table in `/data/values.csv` has 3 fields per row, but 1 row had another number, so it was skipped: row 6 has 1 field"
        );
        assert_eq!(reported[0].occurrences, 1);
        assert_eq!(reported[0].defining, [ContextField::Source(Input::Local(PathBuf::from("/data/values.csv")))]);
        assert_eq!(reported[0].incidental, []);
    }

    #[test]
    fn a_downloaded_file_is_named_by_its_url() {
        let sink = RecordingReporter::new();
        let source = Input::Remote(Url::parse("https://example.org/feed.csv").unwrap());

        report_misshapen_rows(&sink, &source, &rows(&[(5, 1)]));

        let reported = sink.diagnostics();
        assert!(reported[0].headline.starts_with("The table in `https://example.org/feed.csv` has 3 fields"));
        assert_eq!(reported[0].defining, [ContextField::Source(source)]);
    }

    #[test]
    fn two_files_skipping_the_same_rows_are_two_findings_each_naming_its_file() {
        let sink = RecordingReporter::new();

        report_misshapen_rows(&sink, &Input::Local(PathBuf::from("/data/monday.csv")), &rows(&[(5, 1)]));
        report_misshapen_rows(&sink, &Input::Local(PathBuf::from("/data/tuesday.csv")), &rows(&[(5, 1)]));

        let reported = sink.diagnostics();
        assert_eq!(reported[0].defining, [ContextField::Source(Input::Local(PathBuf::from("/data/monday.csv")))]);
        assert_eq!(reported[1].defining, [ContextField::Source(Input::Local(PathBuf::from("/data/tuesday.csv")))]);
    }

    #[test]
    fn many_skipped_rows_are_one_warning_naming_the_first_and_counting_every_row() {
        let sink = RecordingReporter::new();

        let total = report_misshapen_rows(
            &sink,
            &Input::Local(PathBuf::from("/data/values.csv")),
            &rows(&[(2, 2), (4, 4), (6, 2), (8, 2), (9, 1)]),
        );

        let reported = sink.diagnostics();
        assert_eq!(total, 5);
        assert_eq!(reported.len(), 1);
        assert_eq!(
            reported[0].headline,
            "The table in `/data/values.csv` has 3 fields per row, but 5 rows had another number, so they were skipped: row 3 has 2 fields; row 5 has 4 fields; row 7 has 2 fields; and 2 more"
        );
        assert_eq!(reported[0].occurrences, 5);
    }
}
