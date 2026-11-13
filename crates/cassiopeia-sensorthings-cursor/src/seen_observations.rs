use cassiopeia_sensorthings::entity_id::EntityId;
use chrono::{DateTime, Utc};
use derive_more::From;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Written Observations by id, each with the reading time it was written for: the Observations a
/// position has seen, or those one cycle wrote for a data stream.
///
/// A late upload carries an earlier `phenomenonTime` than readings already written, and no entity
/// carries an insertion time (OGC 18-088 section 8.1, Table 2), so only its id tells it apart from a
/// reading written before.
#[derive(Debug, Clone, PartialEq, Eq, Default, From, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SeenObservations(BTreeMap<EntityId, DateTime<Utc>>);

impl SeenObservations {
    /// Adds the Observation `id`, written for the reading time `time`.
    pub fn insert(&mut self, id: EntityId, time: DateTime<Utc>) {
        self.0.insert(id, time);
    }

    /// Whether the Observation `id` is among these.
    #[must_use]
    pub fn contains(&self, id: &EntityId) -> bool {
        self.0.contains_key(id)
    }

    /// The latest reading time among these, or `None` when there are none.
    #[must_use]
    pub fn latest(&self) -> Option<DateTime<Utc>> {
        self.0.values().max().copied()
    }

    /// How many Observations these are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Adds every Observation of `written`.
    pub(crate) fn join(&mut self, written: SeenObservations) {
        self.0.extend(written.0);
    }

    /// Keeps only the Observations whose reading time is at or after `earliest`.
    pub(crate) fn keep_from(&mut self, earliest: DateTime<Utc>) {
        self.0.retain(|_, time| *time >= earliest);
    }
}

#[cfg(test)]
mod tests {
    use crate::seen_observations::SeenObservations;
    use cassiopeia_sensorthings::entity_id::EntityId;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    fn seen(observations: &[(i64, &str)]) -> SeenObservations {
        SeenObservations::from(
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
        assert_eq!(SeenObservations::default().latest(), None);
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
