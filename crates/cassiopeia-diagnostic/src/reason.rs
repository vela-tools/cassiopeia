use crate::{code::diagnostic_code::DiagnosticCode, severity::Severity};
use getset::{CopyGetters, Getters};

/// Whether a diagnostic had been seen before.
///
/// The deduplicating middleware forwards a [`Sighting::First`] to the backend and swallows every
/// [`Sighting::Repeat`], so a hundred identical failures cost one line plus a counter.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Sighting {
    /// Nothing with this identity had been recorded yet.
    First,
    /// An identical failure was already recorded.
    Repeat,
}

/// One row of the run summary's reason table: a code, how often it fired, and one example.
#[derive(Clone, CopyGetters, Debug, Eq, Getters, PartialEq)]
pub struct Reason {
    /// How serious the failures under this code were.
    #[getset(get_copy = "pub")]
    severity: Severity,
    /// The code the failures share.
    #[getset(get_copy = "pub")]
    code: DiagnosticCode,
    /// How many occurrences the code accounted for.
    #[getset(get_copy = "pub")]
    count: u64,
    /// One representative explanation.
    example: Box<str>,
}

impl Reason {
    /// Assembles one row.
    #[must_use]
    pub const fn new(severity: Severity, code: DiagnosticCode, count: u64, example: Box<str>) -> Reason {
        Reason {
            severity,
            code,
            count,
            example,
        }
    }

    /// The representative explanation.
    #[must_use]
    pub const fn example(&self) -> &str {
        &self.example
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        code::{diagnostic_code::DiagnosticCode, schema_code::SchemaCode},
        reason::{Reason, Sighting},
        severity::Severity,
    };

    #[test]
    fn the_two_sightings_are_distinct() {
        assert_ne!(Sighting::First, Sighting::Repeat);
    }

    #[test]
    fn a_reason_carries_its_code_count_and_example() {
        let reason = Reason::new(
            Severity::Warning,
            DiagnosticCode::Schema(SchemaCode::Nonconformant),
            18,
            "/temperature: required property missing".into(),
        );

        assert_eq!(reason.code().to_string(), "schema-nonconformant");
        assert_eq!(reason.count(), 18);
        assert_eq!(reason.example(), "/temperature: required property missing");
    }
}
