use crate::{error::ValidatorError, report::ValidationReportEntry};

/// How much information a schema check should produce for a nonconformant entity.
///
/// Conformant and absent-schema outcomes never construct diagnostics, regardless of this setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticsLevel {
    /// Return only the three-state verdict.
    None,
    /// Include human-readable validation errors for a pipeline failure.
    Errors,
    /// Include both human-readable errors and JSON Schema list output for a report.
    Report,
}

/// The outcome of checking one entity against its JSON Schema.
///
/// The three states are kept distinct so the pipeline stage can apply a validation policy that
/// treats a made-up data model (no schema) differently from a schema-backed violation. The
/// validator renders the verdict; it never decides what a verdict means for the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaVerdict {
    /// No schema file exists for this entity type; nothing was checked.
    Absent,

    /// A schema exists and the entity conforms.
    Conformant,

    /// A schema exists and the entity violates it.
    Nonconformant,
}

/// Diagnostics attached to a nonconformant verdict when the caller requested them.
#[derive(Debug)]
pub enum ValidationDiagnostics {
    /// The caller requested only a verdict, so no additional validation pass was performed.
    None,
    /// Human-readable errors were requested for a pipeline failure.
    Errors {
        /// The typed validation failure.
        error: Box<ValidatorError>,
    },
    /// Human-readable errors and structured JSON Schema list output were requested.
    Report {
        /// The typed validation failure.
        error: Box<ValidatorError>,
        /// The report entry in JSON Schema list output format.
        entry: ValidationReportEntry,
    },
}

/// The verdict and any diagnostics requested for one entity.
///
/// Constructors enforce that absent and conformant outcomes never carry diagnostics and that only a
/// nonconformant outcome can carry error or report details.
#[derive(Debug)]
pub struct ValidationOutcome {
    verdict: SchemaVerdict,
    diagnostics: ValidationDiagnostics,
}

impl ValidationOutcome {
    /// Creates an absent-schema outcome without diagnostics.
    #[must_use]
    pub const fn absent() -> ValidationOutcome {
        ValidationOutcome {
            verdict: SchemaVerdict::Absent,
            diagnostics: ValidationDiagnostics::None,
        }
    }

    /// Creates a conformant outcome without diagnostics.
    #[must_use]
    pub const fn conformant() -> ValidationOutcome {
        ValidationOutcome {
            verdict: SchemaVerdict::Conformant,
            diagnostics: ValidationDiagnostics::None,
        }
    }

    /// Creates a nonconformant outcome with exactly the diagnostics the caller requested.
    #[must_use]
    pub const fn nonconformant(diagnostics: ValidationDiagnostics) -> ValidationOutcome {
        ValidationOutcome {
            verdict: SchemaVerdict::Nonconformant,
            diagnostics,
        }
    }

    /// Returns the three-state schema verdict.
    #[must_use]
    pub const fn verdict(&self) -> SchemaVerdict {
        self.verdict
    }

    /// Consumes the outcome and returns its diagnostics.
    #[must_use]
    pub fn into_diagnostics(self) -> ValidationDiagnostics {
        self.diagnostics
    }
}
