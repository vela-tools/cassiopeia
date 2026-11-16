use cassiopeia_collector::sensorthings::unread_check::UnreadCheck;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether a run has made its check for the `MultiDatastreams` its SensorThings inputs read none of.
///
/// A run makes the check once, in the cycle that starts first, so a scheduled run asks each of its
/// servers at most once whatever its schedule. The collector is built anew every cycle, so the run
/// keeps this beside its cursor stores.
#[derive(Debug, Default)]
pub(crate) struct UnreadChecks {
    /// Whether a cycle has claimed the check.
    claimed: AtomicBool,
}

impl UnreadChecks {
    /// Whether the cycle about to start makes the check: the first cycle to ask does, and every later
    /// one does not.
    pub(crate) fn claim(&self) -> UnreadCheck {
        if self.claimed.swap(true, Ordering::AcqRel) {
            UnreadCheck::Done
        } else {
            UnreadCheck::Due
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::unread_checks::UnreadChecks;
    use cassiopeia_collector::sensorthings::unread_check::UnreadCheck;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_the_first_cycle_makes_the_check() {
        let checks = UnreadChecks::default();

        assert_eq!(
            [checks.claim(), checks.claim(), checks.claim()],
            [UnreadCheck::Due, UnreadCheck::Done, UnreadCheck::Done]
        );
    }
}
