use crate::{discarded_observations::DiscardedObservations, merged_records::MergedRecords};

/// The records an assembly scan found lost, one tally per reason.
///
/// Each reason is a different problem with a different fix, so each keeps its own tally and is
/// reported under its own code; they travel together because both are read off the same stored
/// fragments, in the same scan, while the store hands them over for the one and only time.
#[derive(Debug, Default)]
pub struct AssemblyLosses {
    /// Records of one mapping that resolved to one id and disagreed on a field.
    pub merged: MergedRecords,
    /// Records a current-state store discarded because their `observedAt` template failed.
    pub discarded: DiscardedObservations,
}

impl AssemblyLosses {
    /// Opens a set of empty tallies.
    #[must_use]
    pub fn new() -> AssemblyLosses {
        AssemblyLosses::default()
    }
}

#[cfg(test)]
mod tests {
    use crate::assembly_losses::AssemblyLosses;

    #[test]
    fn a_fresh_set_of_tallies_holds_nothing() {
        let losses = AssemblyLosses::new();

        assert!(losses.merged.is_empty());
        assert!(losses.discarded.is_empty());
    }
}
