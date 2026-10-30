use crate::recurring::Recurring;
use derive_where::derive_where;
use indexmap::{Equivalent, IndexMap, map::IntoIter};
use parking_lot::Mutex;
use std::hash::Hash;

/// The failures one batch met, one record of type `R` per key of type `K`.
///
/// The first failure a key meets opens its record; every later one is counted against that same
/// record rather than opening another, so the sink stays bounded by its keys however many records
/// fail. Which failure opens a record, when a batch is processed in parallel, is whichever reached
/// the sink first: its example illustrates the failure, it does not identify a record.
///
/// The sink is shared, not owned per record: a batch is processed across a Rayon pool, so every
/// worker records into the same one behind a shared reference.
///
/// The map is an [`IndexMap`] behind a [`Mutex`] rather than a `DashMap`: a run's messages come out
/// in the order the failures were met rather than in a hash order that would reshuffle between
/// builds, and a sink is built per batch and stays empty in almost every one, so a sharded map's
/// per-CPU allocation would be paid for nothing. The lock is only ever taken on the failure path.
#[derive_where(Default)]
pub struct BatchSink<K, R> {
    entries: Mutex<IndexMap<K, R>>,
}

impl<K, R> BatchSink<K, R> {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> BatchSink<K, R> {
        BatchSink::default()
    }

    /// Whether nothing failed, which is the common case for a batch.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Every key that failed and its record, in the order they first failed.
    #[must_use]
    pub fn into_entries(self) -> IntoIter<K, R> {
        self.entries.into_inner().into_iter()
    }
}

impl<K: Hash + Eq, R: Recurring> BatchSink<K, R> {
    /// Counts one more failure against the record `key` finds, or files the key and record `open`
    /// returns when it finds none.
    ///
    /// The key is looked up borrowed, so only the first failure of a key pays for the owned key the
    /// map files its record under.
    pub fn tally<Q>(&self, key: &Q, open: impl FnOnce() -> (K, R))
    where
        Q: ?Sized + Hash + Equivalent<K>,
    {
        let mut entries = self.entries.lock();
        if let Some(record) = entries.get_mut(key) {
            record.count_another();
            return;
        }

        let (key, record) = open();
        entries.insert(key, record);
    }

    /// Counts one more failure against the record `key` finds, or files `key` with the record
    /// `first` returns when it finds none.
    ///
    /// For a key the caller has to build owned anyway, such as one that carries the failure itself.
    pub fn tally_owned(&self, key: K, first: impl FnOnce() -> R) {
        let mut entries = self.entries.lock();
        if let Some(record) = entries.get_mut(&key) {
            record.count_another();
            return;
        }

        entries.insert(key, first());
    }
}

#[cfg(test)]
mod tests {
    use crate::{batch_sink::BatchSink, occurrences::Occurrences, recurring::Recurring};
    use pretty_assertions::assert_eq;

    fn counts(sink: BatchSink<&'static str, Occurrences>) -> Vec<(&'static str, u64)> {
        sink.into_entries().map(|(key, record)| (key, record.occurrences().get())).collect()
    }

    #[test]
    fn a_fresh_sink_holds_nothing() {
        assert!(BatchSink::<&str, Occurrences>::new().is_empty());
    }

    #[test]
    fn repeats_of_one_key_are_counted_against_its_first_record() {
        let sink = BatchSink::new();
        sink.tally("location", || ("location", Occurrences::first()));
        sink.tally("location", || ("location", Occurrences::first()));
        sink.tally_owned("location", Occurrences::first);

        assert_eq!(counts(sink), [("location", 3)]);
    }

    #[test]
    fn distinct_keys_are_distinct_records_in_the_order_they_first_failed() {
        let sink = BatchSink::new();
        sink.tally_owned("area", Occurrences::first);
        sink.tally("location", || ("location", Occurrences::first()));
        sink.tally_owned("area", Occurrences::first);

        assert_eq!(counts(sink), [("area", 2), ("location", 1)]);
    }
}
