use crate::{multi_datastream_elements::ElementCounts, stream_id::StreamId, stream_kind::StreamKind};
use thiserror::Error;

/// Why a served data stream names no reading the server offers.
///
/// Each is a data stream OGC 18-088 does not allow: every data stream has an id and exactly one
/// Thing (sections 8.1 and 8.2.4), a Datastream exactly one `ObservedProperty` and a
/// `MultiDatastream` one or more, each with a `name` (sections 8.2.6 and 12), and a
/// `MultiDatastream` one unit of measurement and one observation type per `ObservedProperty`
/// (section 12).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum OfferedReadingError {
    /// The data stream has no `@iot.id`, or one that is neither an integer nor a text.
    #[error("the server sent a {} without a readable `@iot.id`: an id is an integer or a text", .0.entity())]
    IdUnreadable(StreamKind),

    /// The data stream came without its Thing, or with a Thing whose id cannot be read, so nothing
    /// tells which Thing holds it.
    #[error("the server sent {datastream} without its Thing, or with a Thing without a readable `@iot.id`")]
    ThingUnreadable {
        /// The data stream.
        datastream: StreamId,
    },

    /// The data stream came without an `ObservedProperty` that has a `name`, so its readings have no
    /// name.
    #[error("the server sent {datastream} without an ObservedProperty that has a `name`, so its readings have no name")]
    ObservedPropertyUnnamed {
        /// The data stream.
        datastream: StreamId,
    },

    /// The `MultiDatastream` holds `ObservedProperties`, units of measurement and observation types
    /// that differ in number, so the values of its results cannot be read by position.
    #[error("the server sent {datastream} with {counts}, so the values of its results cannot be read by position")]
    ElementCountsDiffer {
        /// The `MultiDatastream`.
        datastream: StreamId,
        /// How many of each it holds.
        counts: ElementCounts,
    },
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_id::EntityId,
        multi_datastream_elements::ElementCounts,
        offered_readings::error::OfferedReadingError,
        stream_id::StreamId,
        stream_kind::StreamKind,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_refused_data_stream_is_named_by_its_kind_and_id() {
        let error = OfferedReadingError::ObservedPropertyUnnamed {
            datastream: StreamId::Datastream(EntityId::Integer(5)),
        };

        assert_eq!(
            error.to_string(),
            "the server sent Datastream 5 without an ObservedProperty that has a `name`, so its readings have no name"
        );
        assert_eq!(
            OfferedReadingError::IdUnreadable(StreamKind::MultiDatastream).to_string(),
            "the server sent a MultiDatastream without a readable `@iot.id`: an id is an integer or a text"
        );
    }

    #[test]
    fn a_multi_datastream_whose_members_differ_in_number_is_named_with_its_counts() {
        let error = OfferedReadingError::ElementCountsDiffer {
            datastream: StreamId::MultiDatastream(EntityId::Integer(4)),
            counts: ElementCounts {
                observed_properties: 2,
                units: 3,
                observation_types: 2,
            },
        };

        assert_eq!(
            error.to_string(),
            "the server sent MultiDatastream 4 with 2 ObservedProperties, 3 `unitOfMeasurements` and 2 `multiObservationDataTypes`, so the values of its results cannot be read by position"
        );
    }
}
