use crate::position::Position;
use cassiopeia_sensorthings::read_stream::ReadStream;
use derive_more::{From, Into, IntoIterator};
use std::collections::{BTreeMap, btree_map};

/// The read positions a cursor keeps, one per stream, Datastreams first, then `MultiDatastreams`,
/// then Things, each kind by ascending id, so a Datastream, a `MultiDatastream` and a Thing that
/// share an id keep a position each.
#[derive(Debug, Clone, PartialEq, Eq, Default, From, Into, IntoIterator)]
#[into_iterator(ref)]
pub struct Positions(BTreeMap<ReadStream, Position>);

impl Positions {
    /// The position of `stream`, or `None` when the cursor keeps none for it.
    #[must_use]
    pub fn get(&self, stream: &ReadStream) -> Option<&Position> {
        self.0.get(stream)
    }

    /// Every position with its stream, Datastreams first, then `MultiDatastreams`, then Things, each
    /// kind by ascending id.
    pub fn iter(&self) -> btree_map::Iter<'_, ReadStream, Position> {
        self.0.iter()
    }

    /// How many streams the cursor keeps a position for.
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
    use crate::{position::Position, positions::Positions, seen_entities::SeenEntities};
    use cassiopeia_sensorthings::{entity_id::EntityId, read_stream::ReadStream, stream_id::StreamId};
    use chrono::DateTime;
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, time::Duration};

    #[test]
    fn a_position_is_found_by_its_stream_kind_id_and_type() {
        let cycle_start = DateTime::parse_from_rfc3339("2026-11-12T10:00:00Z").unwrap().to_utc();
        let written = SeenEntities::from(BTreeMap::from([(EntityId::Integer(1), cycle_start)]));
        let first = Position::first(written, cycle_start, Duration::from_secs(60)).unwrap();
        let datastream = ReadStream::Data(StreamId::Datastream(EntityId::Integer(5)));
        let positions = Positions::from(BTreeMap::from([(datastream.clone(), first.clone())]));

        assert_eq!(positions.len(), 1);
        assert_eq!(positions.get(&datastream), Some(&first));
        assert_eq!(positions.get(&ReadStream::Data(StreamId::Datastream(EntityId::Text("5".into())))), None);
        assert_eq!(positions.get(&ReadStream::Data(StreamId::MultiDatastream(EntityId::Integer(5)))), None);
        assert_eq!(positions.get(&ReadStream::Thing(EntityId::Integer(5))), None);
        assert_eq!(positions.iter().collect::<Vec<_>>(), [(&datastream, &first)]);
        assert!(Positions::default().is_empty());
    }
}
