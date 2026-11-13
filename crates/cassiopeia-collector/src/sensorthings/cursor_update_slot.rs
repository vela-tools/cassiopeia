use cassiopeia_sensorthings_cursor::cursor_update::CursorUpdate;
use std::sync::{Arc, Mutex, PoisonError};

/// Where an incremental read leaves the update of its cursor once it has read its cycle, for the
/// pipeline to commit once the whole cycle has succeeded.
///
/// The read runs on the collector's thread and the commit on the pipeline's, so both hold the slot.
/// A read that fails leaves it empty.
#[derive(Debug, Clone, Default)]
pub struct CursorUpdateSlot(Arc<Mutex<Option<CursorUpdate>>>);

impl CursorUpdateSlot {
    /// Leaves `update` in the slot, in place of any update left before.
    pub fn fill(&self, update: CursorUpdate) {
        // A thread that panicked while holding the slot left either no update or a whole one.
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(update);
    }

    /// Takes the update the read left, or `None` when it left none.
    #[must_use]
    pub fn take(&self) -> Option<CursorUpdate> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::cursor_update_slot::CursorUpdateSlot;
    use cassiopeia_sensorthings_cursor::cursor_update::CursorUpdate;
    use chrono::DateTime;
    use pretty_assertions::assert_eq;
    use std::{thread, time::Duration};

    #[test]
    fn an_update_left_on_another_thread_is_taken_once() {
        let slot = CursorUpdateSlot::default();
        let update = CursorUpdate::new(DateTime::UNIX_EPOCH, Duration::from_secs(3600));
        let reader = slot.clone();
        let left = update.clone();

        thread::spawn(move || reader.fill(left)).join().unwrap();

        assert_eq!(slot.take(), Some(update));
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn a_slot_nothing_was_left_in_is_empty() {
        assert_eq!(CursorUpdateSlot::default().take(), None);
    }
}
