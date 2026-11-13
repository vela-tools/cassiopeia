use crate::{
    error::Result,
    input_config::{InputConfig, LaneSource},
};
use cassiopeia_collector::sensorthings::{cursor_cycle::CursorCycle, cursor_update_slot::CursorUpdateSlot};
use cassiopeia_sensorthings::cursor_name::CursorName;
use cassiopeia_sensorthings_cursor::{cursor_store::CursorStore, positions::Positions};
use std::{collections::BTreeMap, path::Path};

/// The cursor stores of a run's incremental inputs, by cursor name.
///
/// Every store is opened when the run is built and held until the run ends, scheduled cycles
/// included, so another run of the same manifest, or a cursor reset, is refused meanwhile. A store
/// that cannot be opened fails the run before it sends any request.
#[derive(Debug)]
pub(crate) struct CursorStores(BTreeMap<CursorName, CursorStore>);

/// The cursors one cycle reads through: the positions each store committed before the cycle, until
/// its lane claims them, and the slot each read leaves its update in.
#[derive(Debug, Default)]
pub(crate) struct CycleCursors {
    /// The positions no lane has claimed yet, by cursor.
    unclaimed: BTreeMap<CursorName, Positions>,
    /// The slot each read leaves its update in, by cursor.
    slots: BTreeMap<CursorName, CursorUpdateSlot>,
}

impl CursorStores {
    /// Opens, in `folder`, the store of every input among `inputs` that reads incrementally, for the
    /// server it reads.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::Cursor`](crate::error::PipelineError::Cursor) for the first store
    /// that cannot be opened, as when another run holds it or it keeps the positions of another
    /// server.
    pub(crate) fn open(inputs: &[InputConfig], folder: &Path) -> Result<CursorStores> {
        let stores = inputs
            .iter()
            .filter_map(|input| input.source.incremental_read())
            .map(|(service, read)| {
                // The store and the map each own the name, and the store the root, while the lane keeps
                // its own.
                let store = CursorStore::open(folder, read.cursor().clone(), service.root().clone())?;
                Ok((read.cursor().clone(), store))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(CursorStores(stores))
    }

    /// The cursors of the cycle about to start, each holding the positions its store holds now.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::Cursor`](crate::error::PipelineError::Cursor) when a store's
    /// positions cannot be read.
    pub(crate) fn start_cycle(&self) -> Result<CycleCursors> {
        let mut cycle = CycleCursors::default();
        for (cursor, store) in &self.0 {
            // The cycle names each cursor in two maps, while the stores keep their own name.
            cycle.unclaimed.insert(cursor.clone(), store.positions()?);
            cycle.slots.insert(cursor.clone(), CursorUpdateSlot::default());
        }
        Ok(cycle)
    }

    /// Commits the update every read of `cycle` left, each store in one write transaction.
    ///
    /// A cursor whose read left no update stays where it was.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::Cursor`](crate::error::PipelineError::Cursor) for the first store
    /// whose commit fails; the stores before it stay committed.
    pub(crate) fn commit(&self, cycle: CycleCursors) -> Result<()> {
        for (cursor, slot) in cycle.slots {
            if let (Some(store), Some(update)) = (self.0.get(&cursor), slot.take()) {
                store.commit(update)?;
            }
        }
        Ok(())
    }
}

impl CycleCursors {
    /// The cursor of the lane that reads `source` this cycle, if it reads incrementally: the
    /// positions to resume from and the slot to leave its update in. Each cursor is claimed once.
    pub(crate) fn claim(&mut self, source: &LaneSource) -> Option<CursorCycle> {
        let (_, read) = source.incremental_read()?;
        let positions = self.unclaimed.remove(read.cursor())?;
        // The read and the cycle both hold the slot: the read leaves its update there, the commit
        // takes it.
        let slot = self.slots.get(read.cursor())?.clone();
        Some(CursorCycle::new(positions, slot))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cursor_stores::CursorStores,
        error::PipelineError,
        input_config::{InputConfig, build_input_config},
    };
    use cassiopeia_manifest::input::ManifestInput;
    use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId, service_root::ServiceRoot};
    use cassiopeia_sensorthings_cursor::{
        cursor_store::CursorStore,
        cursor_store_error::CursorStoreError,
        cursor_update::CursorUpdate,
        seen_observations::SeenObservations,
    };
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::Map;
    use std::{collections::BTreeMap, time::Duration};
    use tempfile::TempDir;
    use url::Url;

    /// The lane of an input that reads `root` incrementally through the cursor `cursor`.
    fn incremental(root: &str, cursor: &str) -> InputConfig {
        let json = format!(
            r#"{{"source": "{root}", "format": "sensorthings", "mapping": "m.json5", "sensorthings": {{"read": {{"incremental": {{"cursor": "{cursor}"}}}}}}}}"#
        );
        build_input_config(&serde_json::from_str::<ManifestInput>(&json).unwrap(), &Map::new()).unwrap()
    }

    /// The lane of an input that reads the latest readings of `root`.
    fn latest(root: &str) -> InputConfig {
        let json = format!(r#"{{"source": "{root}", "format": "sensorthings", "mapping": "m.json5"}}"#);
        build_input_config(&serde_json::from_str::<ManifestInput>(&json).unwrap(), &Map::new()).unwrap()
    }

    fn name(cursor: &str) -> CursorName {
        CursorName::try_from(cursor.to_owned()).unwrap()
    }

    fn root(url: &str) -> ServiceRoot {
        ServiceRoot::try_from(Url::parse(url).unwrap()).unwrap()
    }

    /// The update of a cycle that wrote the Observation 1 of data stream 5 at `time`.
    fn update(time: DateTime<Utc>) -> CursorUpdate {
        let mut update = CursorUpdate::new(time, Duration::from_secs(3600));
        update.record(EntityId::Integer(5), SeenObservations::from(BTreeMap::from([(EntityId::Integer(1), time)])));
        update
    }

    #[test]
    fn only_incremental_inputs_open_a_store_and_hold_it() {
        let folder = TempDir::new().unwrap();
        let inputs = [incremental("https://a.example/v1.1", "air"), latest("https://b.example/v1.1")];

        let stores = CursorStores::open(&inputs, folder.path()).unwrap();

        assert!(folder.path().join("air.redb").exists());
        assert_eq!(folder.path().read_dir().unwrap().count(), 1);
        let held = CursorStore::open(folder.path(), name("air"), root("https://a.example/v1.1")).unwrap_err();
        assert!(matches!(held, CursorStoreError::InUse { .. }), "{held:?}");
        drop(stores);
        CursorStore::open(folder.path(), name("air"), root("https://a.example/v1.1")).unwrap();
    }

    #[test]
    fn a_store_of_another_server_fails_the_run() {
        let folder = TempDir::new().unwrap();
        drop(CursorStore::open(folder.path(), name("air"), root("https://other.example/v1.1")).unwrap());

        let error = CursorStores::open(&[incremental("https://a.example/v1.1", "air")], folder.path()).unwrap_err();

        assert!(matches!(&error, PipelineError::Cursor(CursorStoreError::OtherServiceRoot { .. })), "{error:?}");
    }

    #[test]
    fn a_claimed_cursor_s_update_is_committed_and_an_unclaimed_one_changes_nothing() {
        let folder = TempDir::new().unwrap();
        let inputs = [incremental("https://a.example/v1.1", "air"), incremental("https://a.example/v1.1", "noise")];
        let stores = CursorStores::open(&inputs, folder.path()).unwrap();
        let time = DateTime::parse_from_rfc3339("2026-11-12T10:00:00Z").unwrap().to_utc();

        let mut cycle = stores.start_cycle().unwrap();
        let claimed = cycle.claim(&inputs[0].source).unwrap();
        assert!(cycle.claim(&inputs[0].source).is_none());
        assert!(cycle.claim(&latest("https://b.example/v1.1").source).is_none());
        let (positions, slot) = claimed.into_parts();
        assert!(positions.is_empty());
        slot.fill(update(time));
        stores.commit(cycle).unwrap();

        let next = stores.start_cycle().unwrap();
        assert_eq!(next.unclaimed[&name("air")].get(&EntityId::Integer(5)).unwrap().high_water(), time);
        assert!(next.unclaimed[&name("noise")].is_empty());
    }
}
