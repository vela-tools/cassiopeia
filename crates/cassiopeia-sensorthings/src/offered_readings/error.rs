use crate::entity_id::EntityId;
use thiserror::Error;

/// Why a served Datastream names no reading the server offers.
///
/// Each is a Datastream OGC 18-088 does not allow: every Datastream has an id, exactly one Thing
/// and exactly one `ObservedProperty`, which has a `name` (sections 8.1, 8.2.4 and 8.2.6).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum OfferedReadingError {
    /// The Datastream has no `@iot.id`, or one that is neither an integer nor a text.
    #[error("the server sent a Datastream without a readable `@iot.id`: an id is an integer or a text")]
    DatastreamIdUnreadable,

    /// The Datastream came without its Thing, or with a Thing whose id cannot be read, so nothing
    /// tells which Thing holds it.
    #[error("the server sent data stream {datastream} without its Thing, or with a Thing without a readable `@iot.id`")]
    ThingUnreadable {
        /// The data stream's id.
        datastream: EntityId,
    },

    /// The Datastream came without an `ObservedProperty` that has a `name`, so its readings have no
    /// name.
    #[error("the server sent data stream {datastream} without an ObservedProperty that has a `name`, so its readings have no name")]
    ObservedPropertyUnnamed {
        /// The data stream's id.
        datastream: EntityId,
    },
}

#[cfg(test)]
mod tests {
    use crate::{entity_id::EntityId, offered_readings::error::OfferedReadingError};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_refused_data_stream_is_named_by_its_id() {
        let error = OfferedReadingError::ObservedPropertyUnnamed {
            datastream: EntityId::Integer(5),
        };

        assert_eq!(
            error.to_string(),
            "the server sent data stream 5 without an ObservedProperty that has a `name`, so its readings have no name"
        );
    }
}
