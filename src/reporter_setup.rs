use crate::error::Result;
use cassiopeia_common::log::config::LoggerConfig;
use cassiopeia_diagnostic::verbosity::Verbosity;
use cassiopeia_reporter::{
    builder::{ReporterBuilder, ReporterMode},
    global,
    reporter::Reporter,
};

/// Installs the global terminal reporter built from the configured logger and returns a handle to it.
///
/// The verbosity is fixed here, once, for the whole process: nothing downstream threads a level
/// through a call graph or asks a global how much to print.
///
/// # Errors
/// Returns [`Error::Reporter`](crate::error::Error::Reporter) when the reporter cannot be installed.
pub fn init_terminal_reporter(logger: &LoggerConfig, verbosity: Verbosity) -> Result<&'static dyn Reporter> {
    ReporterBuilder::new()
        .with_logger(logger.clone())
        .with_verbosity(verbosity)
        .with_mode(ReporterMode::Terminal)
        .init()?;

    Ok(global::reporter())
}
