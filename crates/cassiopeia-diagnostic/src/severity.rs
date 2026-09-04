use strum::Display;

/// How serious a diagnostic is.
///
/// The variants are declared worst-first so the derived ordering sorts an error ahead of a warning:
/// the run summary's reason table is ordered by severity and reads top-down from the worst.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum Severity {
    /// The run lost data or could not continue.
    Error,
    /// The run carried on, having skipped or degraded something.
    Warning,
}

#[cfg(test)]
mod tests {
    use crate::severity::Severity;

    #[test]
    fn an_error_sorts_ahead_of_a_warning() {
        let mut severities = [Severity::Warning, Severity::Error];
        severities.sort_unstable();

        assert_eq!(severities, [Severity::Error, Severity::Warning]);
    }

    #[test]
    fn each_severity_renders_a_kebab_case_token() {
        assert_eq!(Severity::Error.to_string(), "error");
        assert_eq!(Severity::Warning.to_string(), "warning");
    }
}
