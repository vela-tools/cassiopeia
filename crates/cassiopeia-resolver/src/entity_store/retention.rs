/// Which of a mapping's records resolving to one id an entity store keeps, chosen at construction
/// from the run's temporal target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// The store keeps one entity per id, its current state: a temporal mapping's records observed
    /// last, attribute by attribute, and a static mapping's records merged. A temporal mapping's
    /// records are ranked, so each has its observation times read as it is stored.
    CurrentState,
    /// The store keeps every record as an observation of its own, ranking none of them, so no
    /// observation time is read as a record is stored.
    Series,
}

#[cfg(test)]
mod tests {
    use crate::entity_store::{
        dashmap_latest_store::DashMapLatestEntityStore,
        dashmap_series_store::DashMapSeriesEntityStore,
        redb_latest_store::RedbLatestEntityStore,
        redb_series_store::RedbSeriesEntityStore,
        retention::Retention,
        store::EntityStore,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn each_store_keeps_what_its_kind_names() {
        let stores: [(Box<dyn EntityStore>, Retention); 4] = [
            (Box::new(DashMapLatestEntityStore::new()), Retention::CurrentState),
            (Box::new(RedbLatestEntityStore::new().unwrap()), Retention::CurrentState),
            (Box::new(DashMapSeriesEntityStore::new()), Retention::Series),
            (Box::new(RedbSeriesEntityStore::new().unwrap()), Retention::Series),
        ];
        for (store, retention) in stores {
            assert_eq!(store.retention(), retention, "{store:?}");
        }
    }
}
