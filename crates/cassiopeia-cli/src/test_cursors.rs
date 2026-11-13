//! What the cursor commands' tests share: cursor stores in a temporary cursors folder, moved by the
//! cycles a run would commit.

use cassiopeia_sensorthings::{cursor_name::CursorName, entity_id::EntityId, service_root::ServiceRoot};
use cassiopeia_sensorthings_cursor::{cursor_store::CursorStore, cursor_update::CursorUpdate, seen_observations::SeenObservations};
use chrono::{DateTime, Utc};
use std::{collections::BTreeMap, path::Path, time::Duration};

/// The service root the stores of most tests were read from.
pub(crate) const RIVM: &str = "https://api-samenmeten.rivm.nl/v1.0";

/// The lookback every cycle prunes the Observations it has seen with.
const LOOKBACK: Duration = Duration::from_hours(1);

/// The instant `time` on 2026-11-12, in UTC.
pub(crate) fn at(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("2026-11-12T{time}:00Z")).unwrap().to_utc()
}

pub(crate) fn cursor(name: &str) -> CursorName {
    name.parse().unwrap()
}

/// Opens the store `name` of `folder` for `root`, creating it when there is none.
pub(crate) fn open(folder: &Path, name: &str, root: &str) -> CursorStore {
    let root: ServiceRoot = root.parse().unwrap();
    CursorStore::open(folder, cursor(name), root).unwrap()
}

/// Commits to the store `name` of `folder` for `root` the cycle that started at `cycle_start` and
/// listed the data streams of `listed`, each with the Observations it wrote, by id and time.
pub(crate) fn commit_cycle(folder: &Path, name: &str, root: &str, cycle_start: &str, listed: &[(i64, &[(i64, &str)])]) {
    let mut update = CursorUpdate::new(at(cycle_start), LOOKBACK);
    for (datastream, written) in listed {
        let written = written.iter().map(|(id, time)| (EntityId::Integer(*id), at(time)));
        update.record(EntityId::Integer(*datastream), SeenObservations::from(written.collect::<BTreeMap<_, _>>()));
    }
    open(folder, name, root).commit(update).unwrap();
}
