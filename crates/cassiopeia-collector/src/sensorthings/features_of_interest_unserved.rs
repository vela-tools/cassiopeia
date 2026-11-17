use cassiopeia_sensorthings::{observation_name::ObservationName, service_root::ServiceRoot, stream_id::StreamId};
use derive_more::Display;
use getset::{CopyGetters, Getters};
use std::num::NonZeroU64;

/// An Observation a server served without the `FeatureOfInterest` the read expanded, named with its
/// data stream.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
#[display("{observation} of {datastream}")]
pub struct UnservedObservation {
    /// The Observation.
    pub observation: ObservationName,
    /// The data stream it belongs to.
    pub datastream: StreamId,
}

/// The Observations a read that asked for features of interest wrote without their
/// `FeatureOfInterest`, as one cycle of one server served them: the first, and how many there were.
///
/// Every Observation has exactly one `FeatureOfInterest` (OGC 18-088 section 8.2.7), and an expanded
/// entity is selected implicitly (section 9.3.2.2), so an Observation served without it is one the
/// server left the expansion out of. The read writes it as it was served, and its readings have no
/// `featureOfInterest`.
#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters)]
pub struct FeaturesOfInterestUnserved {
    /// The service root the read was built on.
    #[getset(get = "pub")]
    root: ServiceRoot,
    /// The first Observation the read wrote without its `FeatureOfInterest`.
    #[getset(get = "pub")]
    first: UnservedObservation,
    /// How many Observations the read wrote without their `FeatureOfInterest`, the first included.
    #[getset(get_copy = "pub")]
    count: NonZeroU64,
}

impl FeaturesOfInterestUnserved {
    /// The `count` Observations a read of `root` wrote without their `FeatureOfInterest`, the first
    /// of them `first`.
    #[must_use]
    pub const fn new(root: ServiceRoot, first: UnservedObservation, count: NonZeroU64) -> FeaturesOfInterestUnserved {
        FeaturesOfInterestUnserved { root, first, count }
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::features_of_interest_unserved::UnservedObservation;
    use cassiopeia_sensorthings::{entity_id::EntityId, observation_name::ObservationName, stream_id::StreamId};
    use pretty_assertions::assert_eq;

    #[test]
    fn an_unserved_observation_is_named_with_its_data_stream() {
        let datastream = UnservedObservation {
            observation: ObservationName::Id(EntityId::Integer(70)),
            datastream: StreamId::Datastream(EntityId::Integer(7)),
        };
        let multi_datastream = UnservedObservation {
            observation: ObservationName::WithoutId,
            datastream: StreamId::MultiDatastream(EntityId::Text("ws-4".into())),
        };

        assert_eq!(datastream.to_string(), "Observation 70 of Datastream 7");
        assert_eq!(multi_datastream.to_string(), "an Observation without an id of MultiDatastream ws-4");
    }
}
