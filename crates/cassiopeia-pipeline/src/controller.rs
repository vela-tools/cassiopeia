use std::sync::atomic::{AtomicBool, Ordering};

/// A source of external cancellation for a running pipeline.
///
/// Stage threads poll [`should_cancel`](RunController::should_cancel) between units of work and wind
/// down cooperatively when it returns `true`, so a run stops promptly without tearing threads down.
pub trait RunController: Send + Sync {
    /// Returns `true` once the run should stop at the next checkpoint.
    fn should_cancel(&self) -> bool;
}

/// A [`RunController`] backed by a process-lifetime shutdown flag, the one a signal handler sets.
pub struct AtomicBoolController {
    shutdown: &'static AtomicBool,
}

impl AtomicBoolController {
    /// Wraps the shared shutdown flag as a controller.
    pub const fn new(shutdown: &'static AtomicBool) -> AtomicBoolController {
        AtomicBoolController { shutdown }
    }
}

impl RunController for AtomicBoolController {
    fn should_cancel(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }
}
