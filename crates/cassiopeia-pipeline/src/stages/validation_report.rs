//! Accumulation and serialization of the validator stage's failure report.

use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, run_code::RunCode},
    context_field::ContextField,
    diagnostic_builder::DiagnosticBuilder,
    severity::Severity,
};
use cassiopeia_reporter::reporter::Reporter;
use cassiopeia_validator::report::{ValidationReport, ValidationReportEntry};
use std::{error::Error, fs, mem, path::PathBuf};

/// Accumulates validation failures for the report written once the entity stream ends.
///
/// A collector exists only when the run asked for a report, so it owns the destination path rather
/// than leaving the caller to pair an optional path with an optional accumulator.
pub(crate) struct ReportCollector {
    /// Where the report is written when any entity failed.
    path: PathBuf,
    /// The recorded failure entries, one per entity that failed validation.
    entries: Vec<ValidationReportEntry>,
    /// The count of every entity seen, whether it passed or failed.
    total: u64,
}

impl ReportCollector {
    /// Creates an empty collector writing to `path`.
    pub(crate) const fn new(path: PathBuf) -> ReportCollector {
        ReportCollector {
            path,
            entries: Vec::new(),
            total: 0,
        }
    }

    /// Counts `count` entities as seen, whatever their verdict.
    pub(crate) const fn observe(&mut self, count: u64) {
        self.total = self.total.saturating_add(count);
    }

    /// Records one entity's failure.
    pub(crate) fn record(&mut self, entry: ValidationReportEntry) {
        self.entries.push(entry);
    }

    /// Serializes the collected failures to the report path, if any entity failed.
    ///
    /// A run in which everything conformed writes no file at all, so a stale report from an earlier
    /// run is never mistaken for this run's result.
    pub(crate) fn write(&mut self, reporter: &dyn Reporter) {
        let entries = mem::take(&mut self.entries);
        let failed_entities = u64::try_from(entries.len()).unwrap_or(u64::MAX);
        if failed_entities == 0 {
            return;
        }

        let report = ValidationReport {
            total_entities: self.total,
            failed_entities,
            failures: entries,
        };

        match serde_json::to_string_pretty(&report) {
            Ok(json) => match fs::write(&self.path, json) {
                Ok(()) => reporter.info(&format!("Validation report written to '{}'", self.path.display())),
                Err(error) => self.report_failure(reporter, "The validation report could not be written", &error),
            },
            Err(error) => self.report_failure(reporter, "The validation report could not be serialized", &error),
        }
    }

    /// Reports a failure to produce the report, naming the file it was bound for.
    ///
    /// Losing the report does not lose the run: the entities were already written, so this is an
    /// error about the report itself rather than about the data.
    fn report_failure(&self, reporter: &dyn Reporter, headline: &str, error: &dyn Error) {
        reporter.report(
            &DiagnosticBuilder::new(Severity::Error, DiagnosticCode::Run(RunCode::ReportUnwritable), headline)
                .because(error)
                .with_context(ContextField::SourcePath(self.path.clone()))
                .build(),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::stages::validation_report::ReportCollector;
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use cassiopeia_reporter::backend::noop::NoopReporter;
    use cassiopeia_validator::report::ValidationReportEntry;
    use serde_json::json;
    use std::fs;
    use temp_dir::TempDir;

    /// A stub failure entry for the id given.
    fn entry(id: &str) -> ValidationReportEntry {
        ValidationReportEntry {
            entity_id: id.parse().unwrap(),
            entity_type: NameBuf::new("Sensor").unwrap(),
            evaluation: json!({ "valid": false }),
        }
    }

    #[test]
    fn a_run_with_no_failures_writes_no_report_file() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("report.json");
        let mut collector = ReportCollector::new(path.clone());
        collector.observe(10);

        collector.write(&NoopReporter::new());

        assert!(!path.exists());
    }

    #[test]
    fn a_recorded_failure_is_written_with_the_total_it_was_seen_against() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("report.json");
        let mut collector = ReportCollector::new(path.clone());
        collector.observe(4);
        collector.record(entry("urn:ngsi-ld:Sensor:1"));

        collector.write(&NoopReporter::new());

        let written: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written.get("totalEntities"), Some(&json!(4)));
        assert_eq!(written.get("failedEntities"), Some(&json!(1)));
        assert_eq!(written.get("failures").and_then(serde_json::Value::as_array).map(Vec::len), Some(1));
    }

    #[test]
    fn observing_in_batches_accumulates_the_same_total_as_one_at_a_time() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("report.json");
        let mut collector = ReportCollector::new(path.clone());
        collector.observe(3);
        collector.observe(5);
        collector.record(entry("urn:ngsi-ld:Sensor:2"));

        collector.write(&NoopReporter::new());

        let written: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written.get("totalEntities"), Some(&json!(8)));
    }
}
