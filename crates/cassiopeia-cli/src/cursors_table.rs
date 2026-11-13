use crate::{
    sensorthings_cursors::{CursorPositions, CursorSummary},
    text_table::render_table,
};
use cassiopeia_sensorthings::instant_text::instant_text;
use chrono::{DateTime, Utc};

/// The headings of the cursor listing, in column order.
const LIST_HEADINGS: [&str; 5] = ["cursor", "service root", "data streams", "oldest high water", "newest high water"];

/// The headings of a cursor's positions, in column order.
const POSITION_HEADINGS: [&str; 5] = ["data stream", "high water", "seen", "last listed", "missing since"];

/// What a column shows for an instant a cursor does not hold.
const UNSET: &str = "-";

/// Renders `cursors` as a table: a heading row, then one row per cursor in the order given, with
/// its name, the service root its positions were read from, how many data streams it holds a
/// position for, and the oldest and newest of their high-water marks.
#[must_use]
pub fn render_cursor_list(cursors: &[CursorSummary]) -> String {
    render_table(
        LIST_HEADINGS,
        cursors.iter().map(|summary| {
            let high_water = summary.high_water().as_ref();
            [
                summary.cursor().to_string(),
                summary.root().to_string(),
                summary.datastreams().to_string(),
                instant_or_unset(high_water.map(|span| *span.start())),
                instant_or_unset(high_water.map(|span| *span.end())),
            ]
        }),
    )
}

/// Renders `cursor`: a line naming the service root its positions were read from, then a table of
/// one row per data stream, by ascending id, with its id as `cursor reset --datastream` takes it, its
/// high-water mark, how many Observations it has seen within the lookback, the start of the last
/// cycle that listed it, and the start of the first cycle that no longer listed it.
#[must_use]
pub fn render_cursor_positions(cursor: &CursorPositions) -> String {
    let rows = cursor.positions().iter().map(|(datastream, position)| {
        [
            datastream.to_string(),
            instant_text(position.high_water()),
            position.seen().len().to_string(),
            instant_text(position.last_listed()),
            instant_or_unset(position.missing_since()),
        ]
    });
    format!("service root: {}\n{}", cursor.root(), render_table(POSITION_HEADINGS, rows))
}

/// `instant` as a reading time is written, or [`UNSET`] when there is none.
fn instant_or_unset(instant: Option<DateTime<Utc>>) -> String {
    instant.map_or_else(|| UNSET.to_owned(), instant_text)
}

#[cfg(test)]
mod tests {
    use crate::{
        cursors_table::{render_cursor_list, render_cursor_positions},
        sensorthings_cursors::{list_cursors, show_cursor},
        test_cursors::{RIVM, commit_cycle, cursor},
    };
    use cassiopeia_sensorthings::{entity_id::EntityId, service_root::ServiceRoot};
    use cassiopeia_sensorthings_cursor::{cursor_store::CursorStore, cursor_update::CursorUpdate, seen_observations::SeenObservations};
    use chrono::DateTime;
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, time::Duration};
    use tempfile::TempDir;

    #[test]
    fn a_cursor_with_one_position_lists_one_mark_as_oldest_and_newest() {
        let folder = TempDir::new().unwrap();
        commit_cycle(folder.path(), "a", RIVM, "10:00", &[(5, &[(50, "09:30")])]);

        let listed = render_cursor_list(&list_cursors(folder.path()).unwrap());

        assert_eq!(
            listed.lines().nth(1),
            Some("a       https://api-samenmeten.rivm.nl/v1.0  1             2026-11-12T09:30:00Z  2026-11-12T09:30:00Z")
        );
    }

    #[test]
    fn a_text_id_is_shown_as_reset_takes_it_and_a_fraction_is_kept() {
        let folder = TempDir::new().unwrap();
        let time = DateTime::parse_from_rfc3339("2026-11-12T09:30:00.250Z").unwrap().to_utc();
        let root: ServiceRoot = RIVM.parse().unwrap();
        let store = CursorStore::open(folder.path(), cursor("a"), root).unwrap();
        let mut update = CursorUpdate::new(time, Duration::from_hours(1));
        update.record(
            EntityId::Text("t-5".into()),
            SeenObservations::from(BTreeMap::from([(EntityId::Text("o-1".into()), time)])),
        );
        store.commit(update).unwrap();
        drop(store);

        let shown = render_cursor_positions(&show_cursor(folder.path(), &cursor("a")).unwrap());

        assert_eq!(
            shown.lines().nth(2),
            Some("t-5          2026-11-12T09:30:00.250Z  1     2026-11-12T09:30:00.250Z  -")
        );
    }

    #[test]
    fn a_cursor_without_positions_shows_its_root_and_the_headings_alone() {
        let folder = TempDir::new().unwrap();
        commit_cycle(folder.path(), "a", RIVM, "10:00", &[]);

        assert_eq!(
            render_cursor_positions(&show_cursor(folder.path(), &cursor("a")).unwrap()),
            "service root: https://api-samenmeten.rivm.nl/v1.0\ndata stream  high water  seen  last listed  missing since"
        );
    }
}
