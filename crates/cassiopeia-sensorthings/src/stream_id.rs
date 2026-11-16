use crate::{entity_id::EntityId, stream_kind::StreamKind};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A data stream, named by its kind and its `@iot.id`.
///
/// An id is unique only among the entities of one type (OGC 18-088 section 8.1, Table 2), so a
/// Datastream and a `MultiDatastream` may share one, and only the two together name a data stream.
/// Data streams are ordered Datastreams first, then by id as [`EntityId`]s are. Written as JSON, a
/// data stream is a member named after its kind that holds its id, `"datastream": 56776` or
/// `"multiDatastream": 4`, as an observation line of a snapshot names its data stream.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamId {
    /// A Datastream.
    #[display("Datastream {_0}")]
    Datastream(EntityId),

    /// A `MultiDatastream`.
    #[display("MultiDatastream {_0}")]
    MultiDatastream(EntityId),
}

impl StreamId {
    /// The data stream of kind `kind` whose id is `id`.
    #[must_use]
    pub const fn new(kind: StreamKind, id: EntityId) -> StreamId {
        match kind {
            StreamKind::Datastream => StreamId::Datastream(id),
            StreamKind::MultiDatastream => StreamId::MultiDatastream(id),
        }
    }

    /// The data stream of kind `kind` that `served` is, or `None` when it carries no readable id.
    #[must_use]
    pub fn of(kind: StreamKind, served: &Map<String, Value>) -> Option<StreamId> {
        EntityId::of(served).map(|id| StreamId::new(kind, id))
    }

    /// The data stream's kind.
    #[must_use]
    pub const fn kind(&self) -> StreamKind {
        match self {
            StreamId::Datastream(_) => StreamKind::Datastream,
            StreamId::MultiDatastream(_) => StreamKind::MultiDatastream,
        }
    }

    /// The data stream's id.
    #[must_use]
    pub const fn id(&self) -> &EntityId {
        match self {
            StreamId::Datastream(id) | StreamId::MultiDatastream(id) => id,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_id::EntityId, stream_id::StreamId, stream_kind::StreamKind};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    #[test]
    fn a_data_stream_is_written_as_a_member_named_after_its_kind() {
        assert_eq!(
            serde_json::to_value(StreamId::Datastream(EntityId::Integer(56776))).unwrap(),
            json!({"datastream": 56776})
        );
        assert_eq!(
            serde_json::to_value(StreamId::MultiDatastream(EntityId::Text("ws-7".into()))).unwrap(),
            json!({"multiDatastream": "ws-7"})
        );
        assert_eq!(
            serde_json::from_value::<StreamId>(json!({"multiDatastream": 4})).unwrap(),
            StreamId::MultiDatastream(EntityId::Integer(4))
        );
    }

    #[test]
    fn datastreams_come_first_then_ids_in_order() {
        let mut streams = vec![
            StreamId::MultiDatastream(EntityId::Integer(1)),
            StreamId::Datastream(EntityId::Integer(9)),
            StreamId::Datastream(EntityId::Integer(4)),
        ];
        streams.sort();

        assert_eq!(
            streams,
            [
                StreamId::Datastream(EntityId::Integer(4)),
                StreamId::Datastream(EntityId::Integer(9)),
                StreamId::MultiDatastream(EntityId::Integer(1)),
            ]
        );
    }

    #[test]
    fn a_data_stream_names_its_kind_and_id() {
        let stream = StreamId::new(StreamKind::MultiDatastream, EntityId::Integer(4));

        assert_eq!(stream.to_string(), "MultiDatastream 4");
        assert_eq!(stream.kind(), StreamKind::MultiDatastream);
        assert_eq!(stream.id(), &EntityId::Integer(4));
        assert_eq!(StreamId::Datastream(EntityId::Integer(5)).to_string(), "Datastream 5");
    }

    #[test]
    fn a_served_data_stream_is_named_by_its_id() {
        let Value::Object(served) = json!({"@iot.id": 4, "name": "ws-7"}) else {
            panic!("expected an object");
        };

        assert_eq!(
            StreamId::of(StreamKind::MultiDatastream, &served),
            Some(StreamId::MultiDatastream(EntityId::Integer(4)))
        );
        assert_eq!(StreamId::of(StreamKind::Datastream, &serde_json::Map::new()), None);
    }
}
