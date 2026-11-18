use crate::collection_warnings::CollectionWarnings;

/// How a collection that did not fail ended.
#[derive(Debug, PartialEq, Eq)]
pub enum CollectionEnd {
    /// Every source was collected and handed on, with what collection met that the run carries on
    /// past.
    Collected(CollectionWarnings),
    /// The run's stop ended collection before it was done. A stop is no failure: what was collected
    /// so far is not handed on, and the cycle reports itself stopped.
    Stopped,
}
