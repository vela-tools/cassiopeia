use cassiopeia_sensorthings::{
    entity_id::EntityId,
    multi_datastream_elements::ElementCounts,
    snapshot::{error::SnapshotError, line_number::LineNumber},
    stream_id::StreamId,
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
    /// A `MultiDatastream`.
    #[strum(serialize = "a MultiDatastream")]
    MultiDatastream,
    /// A Thing.
    #[strum(serialize = "a Thing")]
    Thing,
    /// A Location of a Thing.
    #[strum(serialize = "a Location")]
    Location,
    /// The `ObservedProperty` of a Datastream or of an element of a `MultiDatastream`.
    #[strum(serialize = "an ObservedProperty")]
    ObservedProperty,
    /// The `Sensor` of a data stream.
    #[strum(serialize = "a Sensor")]
    Sensor,
    /// An Observation.
    #[strum(serialize = "an Observation")]
    Observation,
    /// The `FeatureOfInterest` of an Observation.
    #[strum(serialize = "a FeatureOfInterest")]
    FeatureOfInterest,
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

    /// A data stream was served without its Thing.
    #[error("line {line} of the snapshot holds {datastream} without its Thing")]
    ThingMissing {
        /// The line the data stream is on.
        line: LineNumber,
        /// The data stream.
        datastream: StreamId,
    },

    /// A data stream was served without an `ObservedProperty`, so its readings cannot be named.
    #[error("{datastream} has no ObservedProperty, so its readings have no name")]
    ObservedPropertyMissing {
        /// The data stream.
        datastream: StreamId,
    },

    /// An `ObservedProperty` of a data stream has no `name`, so its readings cannot be named.
    #[error("{datastream} has an ObservedProperty without a `name`, so its readings have no name")]
    ObservedPropertyUnnamed {
        /// The data stream.
        datastream: StreamId,
    },

    /// A `MultiDatastream` holds `ObservedProperties`, units of measurement and observation types
    /// that differ in number, so the values of its results cannot be read by position.
    #[error("line {line} of the snapshot holds MultiDatastream {multi_datastream} with {counts}, so the values of its results cannot be read by position")]
    ElementCountsDiffer {
        /// The line the `MultiDatastream` is on.
        line: LineNumber,
        /// The `MultiDatastream`'s id.
        multi_datastream: EntityId,
        /// How many of each it holds.
        counts: ElementCounts,
    },

    /// An Observation names a data stream no data stream line holds.
    #[error("line {line} of the snapshot is an Observation of {datastream}, which no data stream line holds")]
    UnknownDatastream {
        /// The observation line's number.
        line: LineNumber,
        /// The data stream the Observation names.
        datastream: StreamId,
    },

    /// A latest snapshot holds an observation line, though it carries each newest Observation in its
    /// data stream line.
    #[error("line {line} of the snapshot is an observation line, but a latest snapshot holds its Observations in its data stream lines")]
    UnexpectedObservation {
        /// The observation line's number.
        line: LineNumber,
    },
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::error::{SensorThingsIngestError, StaEntity};
    use cassiopeia_sensorthings::{entity_id::EntityId, multi_datastream_elements::ElementCounts, snapshot::line_number::LineNumber, stream_id::StreamId};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_missing_observed_property_names_its_data_stream() {
        let error = SensorThingsIngestError::ObservedPropertyMissing {
            datastream: StreamId::Datastream(EntityId::Integer(5)),
        };

        assert_eq!(error.to_string(), "Datastream 5 has no ObservedProperty, so its readings have no name");
    }

    #[test]
    fn a_multi_datastream_whose_members_differ_in_number_is_named_with_its_counts() {
        let error = SensorThingsIngestError::ElementCountsDiffer {
            line: LineNumber::FIRST.next(),
            multi_datastream: EntityId::Integer(4),
            counts: ElementCounts {
                observed_properties: 2,
                units: 3,
                observation_types: 2,
            },
        };

        assert_eq!(
            error.to_string(),
            "line 2 of the snapshot holds MultiDatastream 4 with 2 ObservedProperties, 3 `unitOfMeasurements` and 2 `multiObservationDataTypes`, so the values of its results cannot be read by position"
        );
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
