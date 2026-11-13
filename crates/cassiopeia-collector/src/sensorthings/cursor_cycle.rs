use crate::sensorthings::cursor_update_slot::CursorUpdateSlot;
use cassiopeia_sensorthings_cursor::positions::Positions;

/// What one cycle of an incremental read starts from and leaves behind: the positions its cursor
/// committed before the cycle, and the slot it leaves its update in.
///
/// The collector never opens a store: the pipeline reads the positions before the cycle and commits
/// the update after it, once the cycle has succeeded.
#[derive(Debug)]
pub struct CursorCycle {
    /// The positions the cursor committed before the cycle.
    positions: Positions,
    /// Where the read leaves its update.
    slot: CursorUpdateSlot,
}

impl CursorCycle {
    /// The cycle that resumes from `positions` and leaves its update in `slot`.
    #[must_use]
    pub const fn new(positions: Positions, slot: CursorUpdateSlot) -> CursorCycle {
        CursorCycle { positions, slot }
    }

    /// The positions the cycle resumes from, and the slot it leaves its update in.
    #[must_use]
    pub fn into_parts(self) -> (Positions, CursorUpdateSlot) {
        (self.positions, self.slot)
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{cursor_cycle::CursorCycle, cursor_update_slot::CursorUpdateSlot};
    use cassiopeia_sensorthings_cursor::{cursor_update::CursorUpdate, positions::Positions};
    use chrono::DateTime;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn a_cycle_hands_back_its_positions_and_the_slot_the_pipeline_holds() {
        let slot = CursorUpdateSlot::default();
        let update = CursorUpdate::new(DateTime::UNIX_EPOCH, Duration::from_secs(3600));

        let (positions, left) = CursorCycle::new(Positions::default(), slot.clone()).into_parts();
        left.fill(update.clone());

        assert_eq!(positions, Positions::default());
        assert_eq!(slot.take(), Some(update));
    }
}
