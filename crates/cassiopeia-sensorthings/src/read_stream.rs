use crate::{entity_id::EntityId, stream_id::StreamId, stream_kind::StreamKind};
use derive_more::Display;
use serde::{Deserialize, Serialize};

/// What an incremental read follows from one cycle to the next and keeps a position for: the
/// Observations of a data stream, or the `HistoricalLocations` of a Thing (OGC 18-088 section 8.2.3).
///
/// Streams are ordered data streams first, as [`StreamId`]s are, then Things by ascending id, as
/// [`EntityId`]s are. Written as JSON, a stream is a member named after its kind that holds its id,
/// `"datastream": 56776`, `"multiDatastream": 4` or `"thing": 12512`, so a data stream is written as
/// a [`StreamId`] alone is, and a cursor written before Things were followed reads unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display, Serialize, Deserialize)]
#[serde(from = "StreamMember", into = "StreamMember")]
pub enum ReadStream {
    /// The Observations of a data stream.
    #[display("{_0}")]
    Data(StreamId),

    /// The `HistoricalLocations` of a Thing.
    #[display("Thing {_0}")]
    Thing(EntityId),
}

/// The kind of a [`ReadStream`], displayed in kebab case as the command line writes it:
/// `datastream`, `multi-datastream` or `thing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Display)]
pub enum ReadStreamKind {
    /// A data stream of its kind.
    #[display("{_0}")]
    Data(StreamKind),

    /// A Thing.
    #[display("thing")]
    Thing,
}

/// A stream as it is written: a member named after its kind that holds its id.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum StreamMember {
    /// A Datastream.
    Datastream(EntityId),
    /// A `MultiDatastream`.
    MultiDatastream(EntityId),
    /// A Thing.
    Thing(EntityId),
}

impl ReadStream {
    /// The stream's kind.
    #[must_use]
    pub const fn kind(&self) -> ReadStreamKind {
        match self {
            ReadStream::Data(stream) => ReadStreamKind::Data(stream.kind()),
            ReadStream::Thing(_) => ReadStreamKind::Thing,
        }
    }

    /// The id of the data stream or of the Thing.
    #[must_use]
    pub const fn id(&self) -> &EntityId {
        match self {
            ReadStream::Data(stream) => stream.id(),
            ReadStream::Thing(id) => id,
        }
    }
}

impl ReadStreamKind {
    /// The name of the entity type, as a message names one: `Datastream`, `MultiDatastream` or
    /// `Thing`.
    #[must_use]
    pub const fn entity(self) -> &'static str {
        match self {
            ReadStreamKind::Data(kind) => kind.entity(),
            ReadStreamKind::Thing => "Thing",
        }
    }
}

// Written by hand: no derive maps a member written flat onto a stream that nests its data streams.
impl From<StreamMember> for ReadStream {
    fn from(member: StreamMember) -> ReadStream {
        match member {
            StreamMember::Datastream(id) => ReadStream::Data(StreamId::Datastream(id)),
            StreamMember::MultiDatastream(id) => ReadStream::Data(StreamId::MultiDatastream(id)),
            StreamMember::Thing(id) => ReadStream::Thing(id),
        }
    }
}

// Written by hand: no derive maps a stream that nests its data streams onto the flat member it is
// written as.
impl From<ReadStream> for StreamMember {
    fn from(stream: ReadStream) -> StreamMember {
        match stream {
            ReadStream::Data(StreamId::Datastream(id)) => StreamMember::Datastream(id),
            ReadStream::Data(StreamId::MultiDatastream(id)) => StreamMember::MultiDatastream(id),
            ReadStream::Thing(id) => StreamMember::Thing(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_id::EntityId,
        read_stream::{ReadStream, ReadStreamKind},
        stream_id::StreamId,
        stream_kind::StreamKind,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn a_stream_is_written_as_a_member_named_after_its_kind() {
        let streams = [
            (ReadStream::Data(StreamId::Datastream(EntityId::Integer(56776))), json!({"datastream": 56776})),
            (
                ReadStream::Data(StreamId::MultiDatastream(EntityId::Text("ws-7".into()))),
                json!({"multiDatastream": "ws-7"}),
            ),
            (ReadStream::Thing(EntityId::Integer(12512)), json!({"thing": 12512})),
        ];

        for (stream, written) in streams {
            assert_eq!(serde_json::to_value(&stream).unwrap(), written);
            assert_eq!(serde_json::from_value::<ReadStream>(written).unwrap(), stream);
        }
    }

    #[test]
    fn a_data_stream_is_written_as_its_stream_id_alone_is() {
        let stream = StreamId::MultiDatastream(EntityId::Integer(4));

        assert_eq!(
            serde_json::from_value::<ReadStream>(serde_json::to_value(&stream).unwrap()).unwrap(),
            ReadStream::Data(stream)
        );
    }

    #[test]
    fn data_streams_come_first_then_things_by_id() {
        let mut streams = vec![
            ReadStream::Thing(EntityId::Integer(2)),
            ReadStream::Data(StreamId::MultiDatastream(EntityId::Integer(9))),
            ReadStream::Thing(EntityId::Integer(1)),
            ReadStream::Data(StreamId::Datastream(EntityId::Integer(9))),
        ];
        streams.sort();

        assert_eq!(
            streams,
            [
                ReadStream::Data(StreamId::Datastream(EntityId::Integer(9))),
                ReadStream::Data(StreamId::MultiDatastream(EntityId::Integer(9))),
                ReadStream::Thing(EntityId::Integer(1)),
                ReadStream::Thing(EntityId::Integer(2)),
            ]
        );
    }

    #[test]
    fn a_stream_names_its_kind_and_id() {
        let thing = ReadStream::Thing(EntityId::Text("bus-12".into()));
        let multi = ReadStream::Data(StreamId::MultiDatastream(EntityId::Integer(4)));

        assert_eq!(thing.to_string(), "Thing bus-12");
        assert_eq!(thing.kind(), ReadStreamKind::Thing);
        assert_eq!(thing.id(), &EntityId::Text("bus-12".into()));
        assert_eq!(multi.to_string(), "MultiDatastream 4");
        assert_eq!(multi.kind(), ReadStreamKind::Data(StreamKind::MultiDatastream));
        assert_eq!(multi.id(), &EntityId::Integer(4));
    }

    #[test]
    fn a_kind_is_displayed_as_the_command_line_writes_it_and_names_its_entity() {
        assert_eq!(ReadStreamKind::Data(StreamKind::Datastream).to_string(), "datastream");
        assert_eq!(ReadStreamKind::Data(StreamKind::MultiDatastream).to_string(), "multi-datastream");
        assert_eq!(ReadStreamKind::Thing.to_string(), "thing");
        assert_eq!(ReadStreamKind::Data(StreamKind::MultiDatastream).entity(), "MultiDatastream");
        assert_eq!(ReadStreamKind::Thing.entity(), "Thing");
    }
}
