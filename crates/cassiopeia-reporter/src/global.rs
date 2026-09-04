//! Global access to the reporter and logger instances.

use crate::{
    backend::noop::NoopReporter,
    error::{ReporterError, Result},
    logging::ReloadHandle,
    reporter::Reporter,
};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use tracing_appender::non_blocking::WorkerGuard;

/// The fallback reporter returned by [`reporter`] before the real one is installed.
static NOOP_REPORTER: NoopReporter = NoopReporter::new();

/// Global reporter instance
static REPORTER: OnceCell<Box<dyn Reporter>> = OnceCell::new();

/// Global reload handle for the logger
static RELOAD_HANDLE: OnceCell<ReloadHandle> = OnceCell::new();

/// Global worker guard for the file appender
static GLOBAL_GUARD: OnceCell<Mutex<Option<WorkerGuard>>> = OnceCell::new();

/// Initializes the global reporter and logger.
///
/// This should only be called once.
///
/// # Errors
/// Returns [`ReporterError::AlreadyInitialized`] if the global reporter was already installed.
pub fn init(reporter: Box<dyn Reporter>, reload_handle: ReloadHandle, worker_guard: Option<WorkerGuard>) -> Result<()> {
    REPORTER.set(reporter).map_err(|_| ReporterError::AlreadyInitialized)?;
    RELOAD_HANDLE.set(reload_handle).map_err(|_| ReporterError::AlreadyInitialized)?;
    GLOBAL_GUARD.set(Mutex::new(worker_guard)).map_err(|_| ReporterError::AlreadyInitialized)?;
    Ok(())
}

/// Returns a reference to the global reporter.
///
/// Falls back to a [`NoopReporter`] when no reporter has been installed, so callers never need to
/// handle an uninitialized reporter. Use [`try_reporter`] to distinguish the two.
#[must_use]
pub fn reporter() -> &'static dyn Reporter {
    REPORTER.get().map_or(&NOOP_REPORTER as &dyn Reporter, |reporter| reporter.as_ref())
}

/// Try to get a reference to the global reporter.
///
/// Returns `None` if the reporter has not been initialized yet.
#[must_use]
pub fn try_reporter() -> Option<&'static dyn Reporter> {
    REPORTER.get().map(|reporter| &**reporter)
}

/// Returns the global reload handle for the logger.
#[must_use]
pub fn reload_handle() -> Option<&'static ReloadHandle> {
    RELOAD_HANDLE.get()
}

/// Replaces the global worker guard.
///
/// # Errors
/// Returns [`ReporterError::Poisoned`] if the guard lock cannot be taken, or
/// [`ReporterError::GuardNotInitialized`] if the reporter was never installed.
pub fn set_worker_guard(guard: Option<WorkerGuard>) -> Result<()> {
    if let Some(mutex) = GLOBAL_GUARD.get() {
        let mut lock = mutex.try_lock().ok_or(ReporterError::Poisoned)?;
        *lock = guard;
        Ok(())
    } else {
        Err(ReporterError::GuardNotInitialized)
    }
}

#[cfg(test)]
mod tests {
    use crate::global::{reporter, try_reporter};

    #[test]
    fn the_global_reporter_is_always_available() {
        // Falls back to the no-op reporter when nothing is installed; never panics.
        reporter().info("ok");
    }

    #[test]
    fn try_reporter_is_none_until_a_reporter_is_installed() {
        // No test in this crate installs a global reporter, so this stays `None`.
        assert!(try_reporter().is_none());
    }
}
