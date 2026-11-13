use crate::error::{CliError, Result};
use cassiopeia_sensorthings::{cursor_name::CursorName, service_root::ServiceRoot, time_point::TimePoint};
use cassiopeia_sensorthings_cursor::{
    cursor_store::CursorStore,
    cursors_folder::sensorthings_stores,
    positions::Positions,
    reset::{ResetScope, ResetTarget},
};
use chrono::{DateTime, Utc};
use getset::{CopyGetters, Getters};
use std::{
    ops::RangeInclusive,
    path::{Path, PathBuf},
};

/// A SensorThings cursor, as `cassiopeia sensorthings cursor list` lists it.
#[derive(Debug, Getters, CopyGetters)]
pub struct CursorSummary {
    /// The cursor.
    #[getset(get = "pub")]
    cursor: CursorName,
    /// The service root its positions were read from.
    #[getset(get = "pub")]
    root: ServiceRoot,
    /// How many data streams it holds a position for.
    #[getset(get_copy = "pub")]
    datastreams: usize,
    /// The oldest and the newest high-water mark of its positions, or `None` when it holds none.
    #[getset(get = "pub")]
    high_water: Option<RangeInclusive<DateTime<Utc>>>,
}

/// The positions a SensorThings cursor keeps, as `cassiopeia sensorthings cursor show` shows them.
#[derive(Debug, Getters)]
#[getset(get = "pub")]
pub struct CursorPositions {
    /// The service root the positions were read from.
    root: ServiceRoot,
    /// The positions, by ascending data stream id.
    positions: Positions,
}

/// Lists the SensorThings cursors in the cursors folder `folder`, by ascending name, leaving out
/// the cursors of other kinds of input.
///
/// # Errors
///
/// Returns [`CliError::Cursor`] when the folder cannot be read, or when a cursor in it cannot be
/// opened or read, a cursor a running job holds among them.
pub fn list_cursors(folder: &Path) -> Result<Vec<CursorSummary>> {
    sensorthings_stores(folder)?.map(|store| CursorSummary::of(&store?)).collect()
}

/// The positions the cursor `cursor` of the cursors folder `folder` keeps.
///
/// # Errors
///
/// Returns [`CliError::Cursor`] when the folder holds no such cursor, or when it cannot be opened or
/// read, as while a running job holds it.
pub fn show_cursor(folder: &Path, cursor: &CursorName) -> Result<CursorPositions> {
    let store = open(folder, cursor)?;
    Ok(CursorPositions {
        positions: store.positions()?,
        // The listing outlives the store it was read from.
        root: store.service_root().clone(),
    })
}

/// Resets the positions of the cursor `cursor` of the cursors folder `folder` within `scope`: removes
/// them without `to`, so their data streams start at the input's initial point again, or sets their
/// high-water marks to the instant `to` names now and forgets what they have seen. Returns the file
/// of the store it changed.
///
/// # Errors
///
/// Returns [`CliError::ResetTargetAfterNow`] when `to` names an instant after now, and
/// [`CliError::Cursor`] when the folder holds no such cursor, when `scope` names a data stream the
/// cursor holds no position for, or when the cursor cannot be opened, read or written, as while a
/// running job holds it. The cursor is then left as it was.
pub fn reset_cursor(folder: &Path, cursor: &CursorName, scope: &ResetScope, to: Option<TimePoint>) -> Result<PathBuf> {
    let target = reset_target(to, Utc::now())?;
    let store = open(folder, cursor)?;
    store.reset(scope, target)?;
    Ok(store.path().to_owned())
}

impl CursorSummary {
    /// The summary of the positions `store` keeps.
    fn of(store: &CursorStore) -> Result<CursorSummary> {
        let positions = store.positions()?;
        let oldest = positions.iter().map(|(_, position)| position.high_water()).min();
        let newest = positions.iter().map(|(_, position)| position.high_water()).max();
        Ok(CursorSummary {
            // The summary outlives the store it was read from.
            cursor: store.cursor().clone(),
            root: store.service_root().clone(),
            datastreams: positions.len(),
            high_water: oldest.zip(newest).map(|(oldest, newest)| oldest..=newest),
        })
    }
}

/// Opens the cursor `cursor` of the cursors folder `folder`, which must exist, for inspection.
fn open(folder: &Path, cursor: &CursorName) -> Result<CursorStore> {
    // The store owns the name of its cursor, while the command line keeps its own.
    Ok(CursorStore::open_existing(folder, cursor.clone())?)
}

/// Where a reset at `now` leaves the positions it applies to: removed without `to`, or with their
/// high-water marks at the instant `to` names.
///
/// A high-water mark after now would make the next cycles skip every reading made before it, so a
/// target after now is refused.
fn reset_target(to: Option<TimePoint>, now: DateTime<Utc>) -> Result<ResetTarget> {
    let Some(point) = to else {
        return Ok(ResetTarget::InitialPoint);
    };
    let high_water = point.at(now);
    if high_water > now {
        return Err(CliError::ResetTargetAfterNow { to: point });
    }
    Ok(ResetTarget::HighWater(high_water))
}

#[cfg(test)]
mod tests {
    use crate::{
        cursors_table::{render_cursor_list, render_cursor_positions},
        error::CliError,
        sensorthings_cursors::{list_cursors, reset_cursor, reset_target, show_cursor},
        test_cursors::{RIVM, at, commit_cycle, cursor, open},
    };
    use cassiopeia_sensorthings::{entity_id::EntityId, time_point::TimePoint};
    use cassiopeia_sensorthings_cursor::reset::{IdText, ResetScope, ResetTarget};
    use pretty_assertions::assert_eq;
    use redb::{Database, TableDefinition};
    use std::{path::Path, time::Duration};
    use tempfile::TempDir;

    /// The other server some stores were read from.
    const FROST: &str = "https://iot.example.org/v1.1";

    /// The message `outcome` failed with.
    fn refusal<T>(outcome: Result<T, CliError>) -> String {
        outcome.err().map(|error| error.to_string()).unwrap_or_default()
    }

    /// The store `rivm-air` of `folder` after two cycles: the first listed the data streams 5 and 9,
    /// and the second listed only 5, so 9 is missing since it started.
    fn rivm_air(folder: &Path) {
        commit_cycle(
            folder,
            "rivm-air",
            RIVM,
            "10:00",
            &[(5, &[(50, "09:30")]), (9, &[(90, "09:40"), (91, "09:45")])],
        );
        commit_cycle(folder, "rivm-air", RIVM, "11:00", &[(5, &[])]);
    }

    /// Makes the store `name` of `folder` record the kind of input `kind`, as another kind of input
    /// records its own.
    fn record_kind(folder: &Path, name: &str, kind: &str) {
        drop(open(folder, name, RIVM));
        let database = Database::open(folder.join(format!("{name}.redb"))).unwrap();
        let transaction = database.begin_write().unwrap();
        transaction
            .open_table(TableDefinition::<&str, &str>::new("metadata"))
            .unwrap()
            .insert("kind", kind)
            .unwrap();
        transaction.commit().unwrap();
    }

    #[test]
    fn cursor_list_shows_every_cursor() {
        let folder = TempDir::new().unwrap();
        commit_cycle(folder.path(), "a", RIVM, "10:00", &[(5, &[(50, "09:30")]), (9, &[(90, "09:40")])]);
        drop(open(folder.path(), "b", FROST));
        record_kind(folder.path(), "c", "other");

        let listed = render_cursor_list(&list_cursors(folder.path()).unwrap());

        assert_eq!(
            listed,
            [
                "cursor  service root                         data streams  oldest high water     newest high water",
                "a       https://api-samenmeten.rivm.nl/v1.0  2             2026-11-12T09:30:00Z  2026-11-12T09:40:00Z",
                "b       https://iot.example.org/v1.1         0             -                     -",
            ]
            .join("\n")
        );
    }

    #[test]
    fn an_empty_or_absent_cursors_folder_lists_the_headings_alone() {
        let folder = TempDir::new().unwrap();

        for listed in [list_cursors(folder.path()), list_cursors(&folder.path().join("cursors"))] {
            assert_eq!(
                render_cursor_list(&listed.unwrap()),
                "cursor  service root  data streams  oldest high water  newest high water"
            );
        }
    }

    #[test]
    fn cursor_show_prints_every_position() {
        let folder = TempDir::new().unwrap();
        rivm_air(folder.path());

        let shown = render_cursor_positions(&show_cursor(folder.path(), &cursor("rivm-air")).unwrap());

        assert_eq!(
            shown,
            [
                "service root: https://api-samenmeten.rivm.nl/v1.0",
                "data stream  high water            seen  last listed           missing since",
                "5            2026-11-12T09:30:00Z  1     2026-11-12T11:00:00Z  -",
                "9            2026-11-12T09:45:00Z  2     2026-11-12T10:00:00Z  2026-11-12T11:00:00Z",
            ]
            .join("\n")
        );
    }

    #[test]
    fn showing_an_absent_cursor_names_it() {
        let folder = TempDir::new().unwrap();

        assert_eq!(
            refusal(show_cursor(folder.path(), &cursor("nope"))),
            format!("cursor 'nope' does not exist in '{}'", folder.path().display())
        );
    }

    #[test]
    fn resetting_a_cursor_in_use_is_refused() {
        let folder = TempDir::new().unwrap();
        rivm_air(folder.path());
        let held = open(folder.path(), "rivm-air", RIVM);
        let before = held.positions().unwrap();

        let refused = reset_cursor(folder.path(), &cursor("rivm-air"), &ResetScope::Store, None);

        assert_eq!(refusal(refused), "cursor 'rivm-air' is in use by a running job");
        drop(held);
        assert_eq!(show_cursor(folder.path(), &cursor("rivm-air")).unwrap().positions(), &before);
    }

    #[test]
    fn listing_or_showing_a_cursor_in_use_is_refused() {
        let folder = TempDir::new().unwrap();
        rivm_air(folder.path());
        let held = open(folder.path(), "rivm-air", RIVM);

        assert_eq!(refusal(list_cursors(folder.path())), "cursor 'rivm-air' is in use by a running job");
        assert_eq!(
            refusal(show_cursor(folder.path(), &cursor("rivm-air"))),
            "cursor 'rivm-air' is in use by a running job"
        );
        drop(held);
    }

    #[test]
    fn resetting_an_unknown_data_stream_names_it() {
        let folder = TempDir::new().unwrap();
        rivm_air(folder.path());
        let before = show_cursor(folder.path(), &cursor("rivm-air")).unwrap();

        let refused = reset_cursor(folder.path(), &cursor("rivm-air"), &ResetScope::Datastream(IdText::from("42".to_owned())), None);

        assert_eq!(refusal(refused), "cursor 'rivm-air' holds no position for the data stream '42'");
        assert_eq!(show_cursor(folder.path(), &cursor("rivm-air")).unwrap().positions(), before.positions());
    }

    #[test]
    fn resetting_one_data_stream_to_an_instant_rewinds_only_it() {
        let folder = TempDir::new().unwrap();
        rivm_air(folder.path());
        let before = show_cursor(folder.path(), &cursor("rivm-air")).unwrap();

        let store = reset_cursor(
            folder.path(),
            &cursor("rivm-air"),
            &ResetScope::Datastream(IdText::from("5".to_owned())),
            Some(TimePoint::Instant(at("08:00"))),
        )
        .unwrap();
        let after = show_cursor(folder.path(), &cursor("rivm-air")).unwrap();
        let rewound = after.positions().get(&EntityId::Integer(5)).unwrap();

        assert_eq!(store, folder.path().join("rivm-air.redb"));
        assert_eq!((rewound.high_water(), rewound.seen().is_empty()), (at("08:00"), true));
        assert_eq!(after.positions().get(&EntityId::Integer(9)), before.positions().get(&EntityId::Integer(9)));
    }

    #[test]
    fn resetting_a_cursor_without_a_target_removes_every_position() {
        let folder = TempDir::new().unwrap();
        rivm_air(folder.path());

        reset_cursor(folder.path(), &cursor("rivm-air"), &ResetScope::Store, None).unwrap();

        assert!(show_cursor(folder.path(), &cursor("rivm-air")).unwrap().positions().is_empty());
    }

    #[test]
    fn a_reset_without_a_target_returns_to_the_initial_point() {
        assert_eq!(reset_target(None, at("10:00")).unwrap(), ResetTarget::InitialPoint);
    }

    #[test]
    fn a_reset_target_is_an_instant_or_a_span_before_now() {
        assert_eq!(
            reset_target(Some(TimePoint::Before(Duration::from_hours(1))), at("10:00")).unwrap(),
            ResetTarget::HighWater(at("09:00"))
        );
        assert_eq!(
            reset_target(Some(TimePoint::Instant(at("10:00"))), at("10:00")).unwrap(),
            ResetTarget::HighWater(at("10:00"))
        );
    }

    #[test]
    fn a_reset_target_after_now_is_refused() {
        let folder = TempDir::new().unwrap();
        rivm_air(folder.path());

        assert_eq!(
            refusal(reset_target(Some(TimePoint::Instant(at("10:01"))), at("10:00"))),
            "`--to 2026-11-12T10:01:00Z` is after now, and a data stream reset to it would skip every reading made before it"
        );
        let refused = reset_cursor(
            folder.path(),
            &cursor("rivm-air"),
            &ResetScope::Store,
            Some("2126-11-12T10:00:00Z".parse().unwrap()),
        );
        assert!(matches!(refused, Err(CliError::ResetTargetAfterNow { .. })), "{refused:?}");
    }
}
