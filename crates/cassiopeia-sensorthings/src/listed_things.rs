use crate::entity_id::EntityId;
use serde_json::{Map, Value};
use std::collections::{BTreeSet, btree_set};

/// The member a served data stream carries its Thing in (OGC 18-088 section 8.2.4).
const THING_MEMBER: &str = "Thing";

/// The Things a read requests the `HistoricalLocations` of, gathered one served data stream at a time
/// from its metadata discoveries: each Thing a listed data stream belongs to, of either kind.
///
/// They are requested by ascending id, each once however many of its data streams the server lists.
/// A data stream served without its Thing, or with a Thing without a readable id, adds none, and the
/// ingestor refuses its line, naming it.
#[derive(Debug, Default)]
pub struct ListedThings {
    /// The Things requested.
    ids: BTreeSet<EntityId>,
}

impl ListedThings {
    /// Adds the Thing of the data stream the server served as `datastream`.
    pub fn add(&mut self, datastream: &Map<String, Value>) {
        if let Some(Value::Object(thing)) = datastream.get(THING_MEMBER)
            && let Some(id) = EntityId::of(thing)
        {
            self.ids.insert(id);
        }
    }

    /// The Things to request, by ascending id.
    #[must_use]
    pub fn into_ids(self) -> btree_set::IntoIter<EntityId> {
        self.ids.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_id::EntityId, listed_things::ListedThings};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    #[test]
    fn things_are_requested_by_ascending_id_once_each() {
        let mut things = ListedThings::default();
        for datastream in [
            json!({"@iot.id": 1, "Thing": {"@iot.id": "bus-12"}}),
            json!({"@iot.id": 2, "Thing": {"@iot.id": 12512, "name": "station-1"}}),
            json!({"@iot.id": 3, "Thing": {"@iot.id": 7}}),
            json!({"@iot.id": 4, "Thing": {"@iot.id": 12512}}),
        ] {
            things.add(&object(datastream));
        }

        assert_eq!(
            things.into_ids().collect::<Vec<_>>(),
            [EntityId::Integer(7), EntityId::Integer(12512), EntityId::Text("bus-12".into())]
        );
    }

    #[test]
    fn a_data_stream_without_a_thing_or_without_a_readable_thing_id_adds_none() {
        let mut things = ListedThings::default();
        for datastream in [
            json!({"@iot.id": 1}),
            json!({"@iot.id": 2, "Thing": null}),
            json!({"@iot.id": 3, "Thing": {"name": "station-1"}}),
            json!({"@iot.id": 4, "Thing": {"@iot.id": 1.5}}),
        ] {
            things.add(&object(datastream));
        }

        assert_eq!(things.into_ids().count(), 0);
    }
}
