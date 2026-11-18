use std::sync::atomic::{AtomicBool, Ordering};

/// Where a cycle stands among the cycles of its run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CycleOrder {
    /// The first cycle of the run to start.
    First,
    /// Any cycle after it.
    Later,
}

/// Which cycle of a run starts first, so the work a run does once, whatever its schedule, is done
/// by that cycle alone.
///
/// A run warns about the keys its mappings declare without effect once, and makes its check for the
/// `MultiDatastreams` its SensorThings inputs read none of once, so a scheduled run asks each of its
/// servers at most once. Every cycle loads its mappings and builds its collectors anew, so the run
/// keeps this beside its cursor stores.
#[derive(Debug, Default)]
pub(crate) struct FirstCycle {
    /// Whether a cycle has claimed the first place.
    claimed: AtomicBool,
}

impl FirstCycle {
    /// Where the cycle about to start stands: the first cycle to ask is the first, and every later
    /// one is later.
    pub(crate) fn claim(&self) -> CycleOrder {
        if self.claimed.swap(true, Ordering::AcqRel) {
            CycleOrder::Later
        } else {
            CycleOrder::First
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::first_cycle::{CycleOrder, FirstCycle};
    use pretty_assertions::assert_eq;

    #[test]
    fn only_the_first_cycle_to_ask_is_the_first() {
        let first_cycle = FirstCycle::default();

        assert_eq!(
            [first_cycle.claim(), first_cycle.claim(), first_cycle.claim()],
            [CycleOrder::First, CycleOrder::Later, CycleOrder::Later]
        );
    }
}
