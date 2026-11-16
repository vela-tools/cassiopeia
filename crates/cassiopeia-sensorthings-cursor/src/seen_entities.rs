use cassiopeia_sensorthings::entity_id::EntityId;
use chrono::{DateTime, Utc};
use derive_more::From;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Written entities by id, each with the time it was written for: the Observations of a data stream
/// with their reading times, or the `HistoricalLocations` of a Thing with their `time`, that a
/// position has seen, or that one cycle wrote for its stream.
///
/// A late upload carries an earlier time than entities already written, and no entity carries an
/// insertion time (OGC 18-088 section 8.1, Table 2), so only its id tells it apart from one written
/// before.
#[derive(Debug, Clone, PartialEq, Eq, Default, From, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SeenEntities(BTreeMap<EntityId, DateTime<Utc>>);

impl SeenEntities {
    /// Adds the entity `id`, written for the time `time`.
    pub fn insert(&mut self, id: EntityId, time: DateTime<Utc>) {
        self.0.insert(id, time);
    }

    /// Whether the entity `id` is among these.
    #[must_use]
    pub fn contains(&self, id: &EntityId) -> bool {
        self.0.contains_key(id)
    }

    /// The latest time among these, or `None` when there are none.
    #[must_use]
    pub fn latest(&self) -> Option<DateTime<Utc>> {
        self.0.values().max().copied()
    }

    /// How many entities these are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Adds every entity of `written`.
    pub(crate) fn join(&mut self, written: SeenEntities) {
        self.0.extend(written.0);
    }

    /// Keeps only the entities whose time is at or after `earliest`.
    pub(crate) fn keep_from(&mut self, earliest: DateTime<Utc>) {
        self.0.retain(|_, time| *time >= earliest);
    }
}

#[cfg(test)]
mod tests {
    use crate::seen_entities::SeenEntities;
    use cassiopeia_sensorthings::entity_id::EntityId;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    fn seen(observations: &[(i64, &str)]) -> SeenEntities {
        SeenEntities::from(
            observations
                .iter()
                .map(|(id, time)| (EntityId::Integer(*id), instant(time)))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn the_latest_reading_time_is_the_greatest_whatever_the_ids() {
        let observations = seen(&[(1, "2026-11-12T09:30:00Z"), (2, "2026-11-12T08:00:00Z")]);

        assert_eq!(observations.latest(), Some(instant("2026-11-12T09:30:00Z")));
        assert_eq!(SeenEntities::default().latest(), None);
    }

    #[test]
    fn joined_observations_are_known_by_id() {
        let mut observations = seen(&[(1, "2026-11-12T09:00:00Z")]);
        observations.join(seen(&[(2, "2026-11-12T09:10:00Z")]));
        observations.insert(EntityId::Text("t-3".into()), instant("2026-11-12T09:20:00Z"));

        assert_eq!(observations.len(), 3);
        assert!(observations.contains(&EntityId::Integer(2)));
        assert!(observations.contains(&EntityId::Text("t-3".into())));
        assert!(!observations.contains(&EntityId::Integer(4)));
    }

    #[test]
    fn observations_before_the_earliest_time_are_dropped_and_those_at_it_kept() {
        let mut observations = seen(&[(1, "2026-11-12T08:30:00Z"), (2, "2026-11-12T09:00:00Z"), (3, "2026-11-12T09:10:00Z")]);
        observations.keep_from(instant("2026-11-12T09:00:00Z"));

        assert_eq!(observations, seen(&[(2, "2026-11-12T09:00:00Z"), (3, "2026-11-12T09:10:00Z")]));
    }
}
