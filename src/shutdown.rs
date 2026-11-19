use crate::error::Result;
use std::sync::atomic::{AtomicBool, Ordering};

/// The process-wide shutdown flag. Pipeline stages poll it to stop between batches.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Installs the handler that requests a graceful shutdown on SIGINT (Ctrl+C), on SIGTERM, which
/// `docker stop`, Kubernetes and systemd send, and on SIGHUP, which a closed terminal sends, and
/// returns the shutdown flag it raises.
///
/// All three raise the one shutdown flag, so a run stops at the same points whichever arrives. The
/// handler replaces each signal's default action of ending the process, so only a command that polls
/// the flag calls this, once: every other command keeps the default action and ends at once.
pub fn watch_stop_signals() -> Result<&'static AtomicBool> {
    ctrlc::set_handler(|| SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst))?;

    Ok(&SHUTDOWN_REQUESTED)
}
