use cassiopeia_common::input::Input;
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_ingestor::geopackage::unreadable_table::UnreadableTable;
use cassiopeia_reporter::reporter::DiagnosticSink;

/// Warns once for each table of a `GeoPackage` the ingest skipped, naming the table and why, and
/// returns how many tables that was.
///
/// One diagnostic per table, since each table has its own reason: a view fails on its missing row id,
/// a table on its coordinate reference system. The file is named by its `source`, so a downloaded one
/// is named by its URL, the same in every run, rather than by the temporary copy it was read from.
pub(crate) fn report_unreadable_tables(sink: &dyn DiagnosticSink, source: &Input, tables: &[UnreadableTable]) -> u64 {
    for unreadable in tables {
        sink.report(
            &DiagnosticBuilder::new(
                Severity::Warning,
                DiagnosticCode::Ingest(IngestCode::TableUnreadable),
                format!("The table `{}` of `{source}` cannot be read, so it was skipped", unreadable.table),
            )
            // The diagnostic outlives the ingestor that names the source.
            .with_context(ContextField::Source(source.clone()))
            .because(&*unreadable.reason)
            .build(),
        );
    }
    u64::try_from(tables.len()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use crate::{stages::unreadable_tables_report::report_unreadable_tables, test_reporter::RecordingReporter};
    use cassiopeia_common::{collection::CollectionName, input::Input};
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, ingest_code::IngestCode},
        context_field::ContextField,
    };
    use cassiopeia_ingestor::geopackage::{error::GeoPackageIngestError, unreadable_table::UnreadableTable};
    use pretty_assertions::assert_eq;
    use std::{path::PathBuf, sync::Arc};

    fn unreadable(table: &str) -> UnreadableTable {
        UnreadableTable {
            table: CollectionName::from(table),
            reason: Arc::new(GeoPackageIngestError::NoGeometryColumn { table: table.to_string() }),
        }
    }

    fn source() -> Input {
        Input::Local(PathBuf::from("/data/vg5000.gpkg"))
    }

    #[test]
    fn a_file_without_skipped_tables_says_nothing_and_counts_nothing() {
        let sink = RecordingReporter::new();

        assert_eq!(report_unreadable_tables(&sink, &source(), &[]), 0);
        assert!(sink.diagnostics().is_empty());
    }

    #[test]
    fn each_skipped_table_is_one_warning_naming_the_table_the_file_and_the_reason() {
        let sink = RecordingReporter::new();

        let total = report_unreadable_tables(&sink, &source(), &[unreadable("v_at_vg"), unreadable("v_gem")]);

        let reported = sink.diagnostics();
        assert_eq!(total, 2);
        assert_eq!(reported.len(), 2);
        assert_eq!(reported[0].code, DiagnosticCode::Ingest(IngestCode::TableUnreadable));
        assert_eq!(reported[0].code.to_string(), "ingest-table-unreadable");
        assert_eq!(
            reported[0].headline,
            "The table `v_at_vg` of `/data/vg5000.gpkg` cannot be read, so it was skipped"
        );
        assert_eq!(
            reported[0].causes,
            ["the GeoPackage table 'v_at_vg' is listed as features, but gpkg_geometry_columns names no geometry column for it"]
        );
        assert_eq!(reported[0].occurrences, 1);
        assert_eq!(reported[0].defining, [ContextField::Source(source())]);
        assert_eq!(
            reported[1].headline,
            "The table `v_gem` of `/data/vg5000.gpkg` cannot be read, so it was skipped"
        );
    }
}
