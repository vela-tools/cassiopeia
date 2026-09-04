//! Fluent builder for creating and configuring reporters and loggers.

use crate::{
    backend::{
        terminal::{reporter::TerminalReporter, style},
        tracing::TracingReporter,
    },
    error::{ReporterError, Result},
    global,
    logging::build_root_layer,
    middleware::dedup::DeduplicatingReporter,
    reporter::Reporter,
};
use cassiopeia_common::log::config::LoggerConfig;
use cassiopeia_diagnostic::verbosity::Verbosity;
use tracing_subscriber::{layer::SubscriberExt, reload::Layer, util::SubscriberInitExt};

/// Which backend the reporter drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReporterMode {
    /// Interactive mode: draws indicatif progress bars and suppresses tracing stdout.
    Terminal,
    /// Server mode: delegates to the tracing framework and enables its outputs.
    Tracing,
}

/// Builder for constructing and installing the global reporter.
pub struct ReporterBuilder {
    logger: LoggerConfig,
    mode: ReporterMode,
    verbosity: Verbosity,
}

impl Default for ReporterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ReporterBuilder {
    /// Creates a builder with default settings: terminal mode, concise output, default logging.
    #[must_use]
    pub fn new() -> ReporterBuilder {
        ReporterBuilder {
            logger: LoggerConfig::default(),
            mode: ReporterMode::Terminal,
            verbosity: Verbosity::Concise,
        }
    }

    /// Sets the reporter backend.
    #[must_use]
    pub const fn with_mode(mut self, mode: ReporterMode) -> Self {
        self.mode = mode;
        self
    }

    /// Loads the logging configuration for the reporter's tracing subscriber.
    #[must_use]
    pub fn with_logger(mut self, logger: LoggerConfig) -> Self {
        self.logger = logger;
        self
    }

    /// Sets how much of each diagnostic the reporter renders.
    #[must_use]
    pub const fn with_verbosity(mut self, verbosity: Verbosity) -> Self {
        self.verbosity = verbosity;
        self
    }

    /// Builds and installs the global reporter and its logging subscriber.
    ///
    /// The deduplicating middleware is always installed: it is what collapses a repeated failure into
    /// one line and what fills the run summary's reason table, so a run without it would both flood
    /// the terminal and lose the table.
    ///
    /// # Errors
    /// Returns a [`ReporterError`] if a progress-style template is invalid, the tracing layer cannot
    /// be built, or the global reporter is already installed.
    pub fn init(mut self) -> Result<()> {
        // In Terminal mode, tracing console and file output are forced off to avoid interfering with
        // the progress bars the terminal backend draws.
        if self.mode == ReporterMode::Terminal {
            style::validate()?;
            self.logger.console.enabled = false;
            self.logger.file.enabled = false;
        }

        let (root_layer, guard) = build_root_layer(&self.logger)?;
        let (reload_layer, handle) = Layer::new(root_layer);

        tracing_subscriber::registry().with(reload_layer).init();

        let backend: Box<dyn Reporter> = match self.mode {
            ReporterMode::Terminal => Box::new(TerminalReporter::new(self.verbosity)),
            ReporterMode::Tracing => Box::new(TracingReporter::new()),
        };

        global::init(Box::new(DeduplicatingReporter::new(backend)), handle, guard)?;

        Ok(())
    }

    /// Re-installs (reloads) the logging subscriber with new configuration.
    ///
    /// # Errors
    /// Returns a [`ReporterError`] if the new tracing layer cannot be built or installed.
    pub fn reload(config: &LoggerConfig) -> Result<()> {
        let (new_layer, new_guard) = build_root_layer(config)?;

        if let Some(handle) = global::reload_handle() {
            handle.reload(new_layer).map_err(ReporterError::Reload)?;
            global::set_worker_guard(new_guard)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::builder::{ReporterBuilder, ReporterMode};
    use cassiopeia_diagnostic::verbosity::Verbosity;

    #[test]
    fn the_builder_defaults_to_terminal_mode_and_concise_output() {
        let builder = ReporterBuilder::new();

        assert_eq!(builder.mode, ReporterMode::Terminal);
        assert_eq!(builder.verbosity, Verbosity::Concise);
    }

    #[test]
    fn the_builder_records_mode_and_verbosity_overrides() {
        let builder = ReporterBuilder::new().with_mode(ReporterMode::Tracing).with_verbosity(Verbosity::Full);

        assert_eq!(builder.mode, ReporterMode::Tracing);
        assert_eq!(builder.verbosity, Verbosity::Full);
    }
}
