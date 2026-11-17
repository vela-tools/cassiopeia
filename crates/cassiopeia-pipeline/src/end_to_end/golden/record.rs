use crate::{end_to_end::harness::RunOutcome, error::PipelineError, test_reporter::RecordedDiagnostic};
use cassiopeia_diagnostic::code::{diagnostic_code::DiagnosticCode, schema_code::SchemaCode};
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::{error::Error, fmt::Display, path::Path};

/// What one golden run wrote and reported, with every path it names replaced by a placeholder, so
/// two runs from different folders compare equal.
#[derive(Debug, PartialEq)]
pub(crate) struct ViewRecord {
    /// Every entity the run wrote, file by file in the order of their names, each file's in the
    /// order it lists them.
    pub(crate) entities: Vec<Value>,
    /// How the run ended and what it reported.
    pub(crate) report: RunReport,
}

/// How a golden run ended and the diagnostics it reported.
#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct RunReport {
    /// The error that ended the run and each of its sources, outermost first, when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<Vec<String>>,
    /// Every diagnostic it reported, ordered by code and message.
    diagnostics: Vec<ReportedDiagnostic>,
}

/// One diagnostic a golden run reported.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ReportedDiagnostic {
    /// Its code.
    #[serde(serialize_with = "serialize_display")]
    code: DiagnosticCode,
    /// How many occurrences it stood for.
    occurrences: u64,
    /// The lines a terminal prints for it in full verbosity: its headline, its cause chain and every
    /// context field, such as the first entity it names.
    message: Vec<String>,
}

/// Writes a value as its display text, so a diagnostic code reads as the token a run summary
/// prints, such as `extractor-number-unreadable`.
fn serialize_display<T: Display, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}

/// The folders a golden run names in its messages, each written as a placeholder instead.
pub(crate) struct Placeholders<'a> {
    /// The case's directory, written as `[case]`.
    pub(crate) case: &'a Path,
    /// The run's own temporary folder, written as `[run]`.
    pub(crate) run: &'a Path,
}

impl Placeholders<'_> {
    /// `text` with every folder replaced by its placeholder.
    fn apply(&self, text: &str) -> String {
        text.replace(&self.case.display().to_string(), "[case]")
            .replace(&self.run.display().to_string(), "[run]")
    }
}

impl ViewRecord {
    /// Records what a golden run wrote and reported, given how it ended.
    pub(crate) fn new(result: Result<(), PipelineError>, outcome: RunOutcome, placeholders: &Placeholders<'_>) -> ViewRecord {
        let failure = result.err().map(|error| {
            let mut chain = Vec::new();
            let mut cause: Option<&dyn Error> = Some(&error);
            while let Some(error) = cause {
                chain.push(placeholders.apply(&error.to_string()));
                cause = error.source();
            }
            chain
        });

        // A case declares no schema, so every entity type it writes reports one; that says nothing
        // about what the mapping wrote.
        let mut diagnostics: Vec<ReportedDiagnostic> = outcome
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.code != DiagnosticCode::Schema(SchemaCode::Absent))
            .map(|diagnostic| ReportedDiagnostic::new(diagnostic, placeholders))
            .collect();
        diagnostics.sort();

        ViewRecord {
            entities: outcome.entities,
            report: RunReport { failure, diagnostics },
        }
    }
}

impl ReportedDiagnostic {
    /// The diagnostic a run reported, with its folders replaced by placeholders.
    fn new(diagnostic: RecordedDiagnostic, placeholders: &Placeholders<'_>) -> ReportedDiagnostic {
        ReportedDiagnostic {
            code: diagnostic.code,
            occurrences: diagnostic.occurrences,
            message: diagnostic.rendered.into_iter().map(|line| placeholders.apply(&line)).collect(),
        }
    }
}
