use cassiopeia_sensorthings::{
    entity_id::EntityId,
    snapshot::{error::SnapshotError, line_number::LineNumber},
};
use strum::Display;
use thiserror::Error;

/// A kind of SensorThings entity a snapshot holds (OGC 18-088 section 8.2), displayed with its
/// article so a message can name one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum StaEntity {
    /// A Datastream.
    #[strum(serialize = "a Datastream")]
    Datastream,
    /// A Thing.
    #[strum(serialize = "a Thing")]
    Thing,
    /// A Location of a Thing.
    #[strum(serialize = "a Location")]
    Location,
    /// The `ObservedProperty` of a Datastream.
    #[strum(serialize = "an ObservedProperty")]
    ObservedProperty,
    /// An Observation.
    #[strum(serialize = "an Observation")]
    Observation,
}

/// Why a SensorThings snapshot cannot be read into records.
#[derive(Debug, Error)]
pub enum SensorThingsIngestError {
    /// The ingestor was handed an in-memory byte payload; it streams a file.
    #[error("the SensorThings ingestor requires a file, not bytes")]
    RequiresFile,

    /// The snapshot file breaks one of its own rules.
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),

    /// An entity has no `@iot.id`, or one that is neither an integer nor a text.
    #[error("line {line} of the snapshot holds {entity} without a readable `@iot.id`: an id is an integer or a text")]
    IdUnreadable {
        /// The line the entity is on.
        line: LineNumber,
        /// The kind of entity.
        entity: StaEntity,
    },

    /// A Datastream was served without its Thing.
    #[error("line {line} of the snapshot holds data stream {datastream} without its Thing")]
    ThingMissing {
        /// The line the data stream is on.
        line: LineNumber,
        /// The data stream's id.
        datastream: EntityId,
    },

    /// A Datastream was served without its `ObservedProperty`, so its readings cannot be named.
    #[error("data stream {datastream} has no ObservedProperty, so its readings have no name")]
    ObservedPropertyMissing {
        /// The data stream's id.
        datastream: EntityId,
    },

    /// A Datastream's `ObservedProperty` has no `name`, so its readings cannot be named.
    #[error("data stream {datastream} has an ObservedProperty without a `name`, so its readings have no name")]
    ObservedPropertyUnnamed {
        /// The data stream's id.
        datastream: EntityId,
    },

    /// An Observation names a data stream no datastream line holds.
    #[error("line {line} of the snapshot is an Observation of data stream {datastream}, which no datastream line holds")]
    UnknownDatastream {
        /// The observation line's number.
        line: LineNumber,
        /// The data stream the Observation names.
        datastream: EntityId,
    },

    /// A latest snapshot holds an observation line, though it carries each newest Observation in its
    /// datastream line.
    #[error("line {line} of the snapshot is an observation line, but a latest snapshot holds its Observations in its datastream lines")]
    UnexpectedObservation {
        /// The observation line's number.
        line: LineNumber,
    },
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::error::{SensorThingsIngestError, StaEntity};
    use cassiopeia_sensorthings::{entity_id::EntityId, snapshot::line_number::LineNumber};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_missing_observed_property_names_its_data_stream() {
        let error = SensorThingsIngestError::ObservedPropertyMissing {
            datastream: EntityId::Integer(5),
        };

        assert_eq!(error.to_string(), "data stream 5 has no ObservedProperty, so its readings have no name");
    }

    #[test]
    fn an_unreadable_id_names_its_line_and_entity() {
        let error = SensorThingsIngestError::IdUnreadable {
            line: LineNumber::FIRST.next(),
            entity: StaEntity::ObservedProperty,
        };

        assert!(
            error.to_string().starts_with("line 2 of the snapshot holds an ObservedProperty without"),
            "{error}"
        );
    }
}
