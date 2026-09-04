use crate::error::Result;
use std::sync::atomic::{AtomicBool, Ordering};

/// The process-wide shutdown flag. Pipeline stages poll it to stop between batches.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Returns the shutdown flag for handing to a pipeline run.
pub fn flag() -> &'static AtomicBool {
    &SHUTDOWN_REQUESTED
}

/// Installs the CTRL+C handler that requests a graceful shutdown.
pub fn install_sync_handler() -> Result<()> {
    ctrlc::set_handler(|| SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst))?;

    Ok(())
}
