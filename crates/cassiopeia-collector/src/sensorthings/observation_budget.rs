use crate::sensorthings::limit_reached::CollectLimit;
use cassiopeia_sensorthings::settings::collect_limits::ObservationLimit;
use std::ops::ControlFlow;

/// How many more Observations one cycle may write, counted up to `limits.observations`.
#[derive(Debug)]
pub(crate) struct ObservationBudget {
    /// The most Observations the cycle writes.
    limit: ObservationLimit,
    /// The Observations it wrote so far.
    written: u64,
}

impl ObservationBudget {
    /// The budget of a cycle that has written nothing, under `limit`.
    pub(crate) const fn new(limit: ObservationLimit) -> ObservationBudget {
        ObservationBudget { limit, written: 0 }
    }

    /// Counts one more Observation as written, or breaks with the limit once the cycle has written as
    /// many as it allows, so the Observation in hand is the first one past it.
    pub(crate) const fn spend(&mut self) -> ControlFlow<CollectLimit> {
        if self.written == self.limit.get() {
            return ControlFlow::Break(CollectLimit::Observations(self.limit));
        }
        self.written += 1;
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{limit_reached::CollectLimit, observation_budget::ObservationBudget};
    use cassiopeia_sensorthings::settings::collect_limits::ObservationLimit;
    use pretty_assertions::assert_eq;
    use std::ops::ControlFlow;

    #[test]
    fn the_budget_allows_as_many_observations_as_its_limit_and_then_breaks() {
        let limit = ObservationLimit::try_from(2).unwrap();
        let mut budget = ObservationBudget::new(limit);

        let spent: Vec<ControlFlow<CollectLimit>> = (0..4).map(|_| budget.spend()).collect();

        assert_eq!(
            spent,
            [
                ControlFlow::Continue(()),
                ControlFlow::Continue(()),
                ControlFlow::Break(CollectLimit::Observations(limit)),
                ControlFlow::Break(CollectLimit::Observations(limit)),
            ]
        );
    }
}
