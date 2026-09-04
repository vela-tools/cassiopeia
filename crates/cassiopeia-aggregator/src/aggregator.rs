use cassiopeia_ngsi_ld::entity::NgsiLdEntity;

/// Folds a stream of single-instance observations into one temporal entity per id.
///
/// The upstream emits an id's observations contiguously, so an aggregator holds one id's fold open at
/// a time: [`offer`](Aggregator::offer) takes one observation and returns the previous id's finished
/// entity when a new id begins, and [`finish`](Aggregator::finish) flushes the last open fold once the
/// stream ends. The `&mut self` shape follows the writer stage: aggregation is inherently stateful.
pub trait Aggregator {
    /// Offers one single-instance observation.
    ///
    /// Returns the previous id's folded [`NgsiLdEntity`] when this observation begins a new id;
    /// otherwise folds it into the open aggregate and returns `None`.
    fn offer(&mut self, entity: NgsiLdEntity) -> Option<NgsiLdEntity>;

    /// Flushes the final open aggregate once the input stream ends, or `None` when nothing is open.
    fn finish(&mut self) -> Option<NgsiLdEntity>;
}
