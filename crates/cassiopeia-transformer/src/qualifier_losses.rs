use crate::unknown_unit_codes::UnknownUnitCodes;
use cassiopeia_unreadable_timestamps::unreadable_timestamps::UnreadableTimestamps;

/// The qualifiers one batch's transformation left off while keeping its attributes, one sink per
/// reason.
///
/// Each reason is a different problem with a different fix, so each keeps its own sink and is
/// reported under its own code; they travel together because every one of them is the same kind of
/// loss, met in the same stage, over the same batch.
#[derive(Default)]
pub struct QualifierLosses {
    /// `observedAt` and `expiresAt` texts that read as no instant (ETSI GS CIM 009 v1.9.1 clauses
    /// 4.5.2.2 and 4.22).
    pub timestamps: UnreadableTimestamps,
    /// `unitCode` texts that are no UN/CEFACT Recommendation No. 20 code (clause 4.5.2).
    pub unit_codes: UnknownUnitCodes,
}

impl QualifierLosses {
    /// Opens a set of empty sinks.
    #[must_use]
    pub fn new() -> QualifierLosses {
        QualifierLosses::default()
    }
}

#[cfg(test)]
mod tests {
    use crate::qualifier_losses::QualifierLosses;

    #[test]
    fn a_fresh_set_of_sinks_holds_nothing() {
        let losses = QualifierLosses::new();

        assert!(losses.timestamps.is_empty());
        assert!(losses.unit_codes.is_empty());
    }
}
