//! `ProgressStyle` builders for each visual state of a stage bar.
//!
//! Every determinate/indeterminate builder wires the live [`column`](crate::backend::terminal::column)
//! trackers into indicatif's custom keys so the elapsed, rate, warning, and latency columns render
//! from shared state. Every template string is validated once via [`validate`] at reporter-build
//! time, so the per-call builders never panic on a malformed template.

use crate::{
    backend::terminal::column::{IndicatifMessageTracker, IndicatifRateTracker, IndicatifTimeTracker, IndicatifWarnTracker},
    error::ReporterError,
    trackers::{rate::RateTracker, time::TimeTracker},
};
use indicatif::ProgressStyle;
use parking_lot::Mutex;
use std::sync::{Arc, atomic::AtomicU64};

/// Tick characters used by every animated (spinner) style.
const SPINNER_TICKS: &str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";

// indicatif's template DSL takes a colour as a name or an Ansi256 code, never a `Style` value, so
// the shared palette (cassiopeia_terminal_style::palette) cannot be referenced directly here. The
// codes below mirror it (`.75` = ACCENT, `.214` = CAUTION, `.241` = FRAME, `.green` = SUCCESS) and
// are the one documented exception to the no-colour-duplication rule.
const WAITING: &str = "  {spinner:.241} {prefix:<16.241} {msg:.241}";
const INDETERMINATE: &str = "  {spinner:.75} {prefix:<16.bold} [{bar:30.75/241}] {msg} {elapsed:<9.241} [{rate:.241}] {latency:.241} {warns}";
const DETERMINATE: &str = "  {spinner:.75} {prefix:<16.bold} [{bar:30.75/241}] {pos:>8}/{len:<8} {elapsed:<9.241} [{rate:.241}] {latency:.241} {warns}";
const WARNING: &str = "  {spinner:.214} {prefix:<16.bold} [{bar:30.214/241}] {pos:>8}/{len:<8} {elapsed:<9.241} [{rate:.241}] {latency:.241} {warns}";
const FINISHED: &str = "  {prefix:.green} {msg:<16} [{bar:30.green/241}] {pos:>8}/{len:<8} {elapsed:<9.241} [{rate:.241}] {latency:.241} {warns}";
const FINISHED_WARNING: &str = "  {prefix:.214} {msg:<16} [{bar:30.214/241}] {pos:>8}/{len:<8} {elapsed:<9.241} [{rate:.241}] {latency:.241} {warns}";
const MESSAGE: &str = "{msg}";
const SPACER: &str = " ";
const PROGRESS: &str = "  {spinner:.75} {prefix:<12.bold} [{bar:30.75/241}] {pos:>8}/{len:<8} {msg} {elapsed:<9.241}";

/// Every template string used by the terminal backend, for build-time validation.
const ALL_TEMPLATES: &[&str] = &[
    WAITING,
    INDETERMINATE,
    DETERMINATE,
    WARNING,
    FINISHED,
    FINISHED_WARNING,
    MESSAGE,
    SPACER,
    PROGRESS,
];

/// Parses a template, mapping a failure to a typed error.
fn parse(template: &str) -> Result<ProgressStyle, ReporterError> {
    ProgressStyle::with_template(template).map_err(ReporterError::InvalidStyleTemplate)
}

/// Builds a style from a known-good template.
///
/// Templates are validated at reporter-build time via [`validate`], so a parse failure here would be
/// a programming error; it falls back to a default bar rather than panicking on a hot path.
fn styled(template: &str) -> ProgressStyle {
    parse(template).unwrap_or_else(|_| ProgressStyle::default_bar())
}

/// Validates every template string, surfacing a malformed one at reporter-build time.
///
/// # Errors
/// Returns [`ReporterError::InvalidStyleTemplate`] naming the first template that fails to parse.
pub(crate) fn validate() -> Result<(), ReporterError> {
    for template in ALL_TEMPLATES {
        parse(template)?;
    }
    Ok(())
}

/// Style for a bare message line (no symbols or bar).
pub(crate) fn message() -> ProgressStyle {
    styled(MESSAGE)
}

/// Style for the blank spacer line inserted before the first message.
pub(crate) fn spacer() -> ProgressStyle {
    styled(SPACER)
}

/// Style for the one-off progress indicator.
pub(crate) fn progress() -> ProgressStyle {
    styled(PROGRESS).tick_chars(SPINNER_TICKS).progress_chars("━╸─")
}

/// Style for a pre-registered stage that has not started yet.
pub(crate) fn waiting() -> ProgressStyle {
    styled(WAITING).tick_chars(SPINNER_TICKS)
}

/// Style for a running stage of unknown length (animated pulse bar).
pub(crate) fn indeterminate(time: TimeTracker, rate: RateTracker, warns: Arc<AtomicU64>, message: Arc<Mutex<String>>) -> ProgressStyle {
    styled(INDETERMINATE)
        .with_key("elapsed", IndicatifTimeTracker(time))
        .with_key("rate", IndicatifRateTracker(rate))
        .with_key("warns", IndicatifWarnTracker(warns))
        .with_key("latency", IndicatifMessageTracker(message))
        .tick_chars(SPINNER_TICKS)
        .progress_chars("━━─")
}

/// Style for a running stage with a known length (filling bar).
pub(crate) fn determinate(time: TimeTracker, rate: RateTracker, warns: Arc<AtomicU64>, message: Arc<Mutex<String>>) -> ProgressStyle {
    styled(DETERMINATE)
        .with_key("elapsed", IndicatifTimeTracker(time))
        .with_key("rate", IndicatifRateTracker(rate))
        .with_key("warns", IndicatifWarnTracker(warns))
        .with_key("latency", IndicatifMessageTracker(message))
        .tick_chars(SPINNER_TICKS)
        .progress_chars("━╸─")
}

/// Style for a successfully finished stage.
pub(crate) fn finished(time: TimeTracker, rate: RateTracker, warns: Arc<AtomicU64>, message: Arc<Mutex<String>>) -> ProgressStyle {
    styled(FINISHED)
        .with_key("elapsed", IndicatifTimeTracker(time))
        .with_key("rate", IndicatifRateTracker(rate))
        .with_key("warns", IndicatifWarnTracker(warns))
        .with_key("latency", IndicatifMessageTracker(message))
        .progress_chars("━━━")
}

/// Style for a running stage that has reported a warning.
pub(crate) fn warning(time: TimeTracker, rate: RateTracker, warns: Arc<AtomicU64>, message: Arc<Mutex<String>>) -> ProgressStyle {
    styled(WARNING)
        .with_key("elapsed", IndicatifTimeTracker(time))
        .with_key("rate", IndicatifRateTracker(rate))
        .with_key("warns", IndicatifWarnTracker(warns))
        .with_key("latency", IndicatifMessageTracker(message))
        .tick_chars(SPINNER_TICKS)
        .progress_chars("━╸─")
}

/// Style for a finished stage that reported at least one warning.
pub(crate) fn finished_warning(time: TimeTracker, rate: RateTracker, warns: Arc<AtomicU64>, message: Arc<Mutex<String>>) -> ProgressStyle {
    styled(FINISHED_WARNING)
        .with_key("elapsed", IndicatifTimeTracker(time))
        .with_key("rate", IndicatifRateTracker(rate))
        .with_key("warns", IndicatifWarnTracker(warns))
        .with_key("latency", IndicatifMessageTracker(message))
        .progress_chars("━━━")
}

#[cfg(test)]
mod tests {
    use crate::backend::terminal::style::validate;

    #[test]
    fn every_template_parses() {
        assert!(validate().is_ok());
    }
}
