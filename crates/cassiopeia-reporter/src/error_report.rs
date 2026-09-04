//! The one place a failure is turned into a report, cause chain and all.

use crate::{backend::terminal::diagnostic_lines::diagnostic_lines, global::try_reporter};
use cassiopeia_diagnostic::{code::diagnostic_code::DiagnosticCode, diagnostic_builder::from_error, severity::Severity, verbosity::Verbosity};
use cassiopeia_terminal_style::rendering::{Stream, detect_rendering};
use std::{
    error::Error,
    io::{Write, stderr},
};

/// Reports a fatal error under `code`, with its full cause chain.
///
/// If the global reporter is installed the diagnostic goes through it, so it lands below the run's
/// bars and at the verbosity the run was started with. Otherwise (a failure during start-up, before
/// the reporter exists) the same diagnostic is rendered by the same function straight to stderr, so
/// the two paths cannot drift apart.
pub fn report_error(code: DiagnosticCode, error: &dyn Error) {
    let diagnostic = from_error(Severity::Error, code, error).build();

    if let Some(reporter) = try_reporter() {
        reporter.report(&diagnostic);
        return;
    }

    let stderr = stderr();
    let mut handle = stderr.lock();
    for line in diagnostic_lines(&diagnostic, Verbosity::Concise, detect_rendering(Stream::Stderr)) {
        let _ = writeln!(handle, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use crate::{backend::terminal::diagnostic_lines::diagnostic_lines, error_report::report_error};
    use cassiopeia_common::error::io::{IoAction, IoError};
    use cassiopeia_diagnostic::{
        code::{diagnostic_code::DiagnosticCode, run_code::RunCode},
        diagnostic_builder::from_error,
        severity::Severity,
        verbosity::Verbosity,
    };
    use cassiopeia_terminal_style::rendering::Rendering;
    use std::{io::Error, path::PathBuf};

    fn failure() -> IoError {
        IoError::FileOperation {
            source: Error::other("permission denied"),
            path: PathBuf::from("/out/AirQualityObserved.json"),
            action: IoAction::Write,
        }
    }

    #[test]
    fn reporting_an_error_with_a_cause_chain_does_not_panic() {
        report_error(DiagnosticCode::Run(RunCode::Failed), &failure());
    }

    #[test]
    fn the_startup_path_writes_the_cause_text_rather_than_an_empty_connector() {
        // The lines the no-reporter branch writes are exactly these, so asserting them here pins the
        // behaviour without capturing the process's stderr.
        let diagnostic = from_error(Severity::Error, DiagnosticCode::Run(RunCode::Failed), &failure()).build();

        let lines = diagnostic_lines(&diagnostic, Verbosity::Concise, Rendering::Plain);

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("AirQualityObserved.json"));
        assert!(lines[1].ends_with("permission denied"));
    }
}
