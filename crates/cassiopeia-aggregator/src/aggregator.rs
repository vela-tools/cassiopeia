use cassiopeia_ngsi_ld::entity::NgsiLdEntity;

/// What offering one observation did to the fold an [`Aggregator`] holds open.
#[derive(Debug, PartialEq)]
pub enum Offered {
    /// Nothing was open, so the observation began the first fold.
    Opened,
    /// The observation shares the open fold's id and was folded into it.
    Joined,
    /// The observation began a new id, which finished the fold that was open: here it is, folded
    /// into one entity, boxed so the variants that carry nothing stay small.
    Finished(Box<NgsiLdEntity>),
}

/// Folds a stream of single-instance observations into one temporal entity per id.
///
/// The upstream emits an id's observations contiguously, so an aggregator holds one id's fold open at
/// a time: [`offer`](Aggregator::offer) takes one observation and hands back the previous id's
/// finished entity when a new id begins, and [`finish`](Aggregator::finish) flushes the last open
/// fold once the stream ends. The `&mut self` shape follows the writer stage: aggregation is
/// inherently stateful.
pub trait Aggregator {
    /// Offers one single-instance observation, and says whether it began the first fold, joined the
    /// open one, or began a new id and so finished the open one.
    fn offer(&mut self, entity: NgsiLdEntity) -> Offered;

    /// Flushes the final open aggregate once the input stream ends, or `None` when nothing is open.
    fn finish(&mut self) -> Option<NgsiLdEntity>;
}
