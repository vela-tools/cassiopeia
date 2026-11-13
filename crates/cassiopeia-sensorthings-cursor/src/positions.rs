use crate::position::Position;
use cassiopeia_sensorthings::entity_id::EntityId;
use derive_more::{From, Into};
use std::collections::BTreeMap;

/// The read positions a cursor keeps, one per data stream, by ascending data stream id.
#[derive(Debug, Clone, PartialEq, Eq, Default, From, Into)]
pub struct Positions(BTreeMap<EntityId, Position>);

impl Positions {
    /// The position of `datastream`, or `None` when the cursor keeps none for it.
    #[must_use]
    pub fn get(&self, datastream: &EntityId) -> Option<&Position> {
        self.0.get(datastream)
    }

    /// How many data streams the cursor keeps a position for.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the cursor keeps no position.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::{position::Position, positions::Positions, seen_observations::SeenObservations};
    use cassiopeia_sensorthings::entity_id::EntityId;
    use chrono::DateTime;
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, time::Duration};

    #[test]
    fn a_position_is_found_by_its_data_stream_id_and_type() {
        let cycle_start = DateTime::parse_from_rfc3339("2026-11-12T10:00:00Z").unwrap().to_utc();
        let written = SeenObservations::from(BTreeMap::from([(EntityId::Integer(1), cycle_start)]));
        let first = Position::first(written, cycle_start, Duration::from_secs(60)).unwrap();
        let positions = Positions::from(BTreeMap::from([(EntityId::Integer(5), first.clone())]));

        assert_eq!(positions.len(), 1);
        assert_eq!(positions.get(&EntityId::Integer(5)), Some(&first));
        assert_eq!(positions.get(&EntityId::Text("5".into())), None);
        assert!(Positions::default().is_empty());
    }
}
