use crate::broker::atomic_writer::AtomicSpoolError;
use cassiopeia_common::error::io::IoError;
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use std::result;
use thiserror::Error;
use url::Url;
use urn_rs::Urn;

/// Failures raised while writing entities to a destination.
#[derive(Debug, Error)]
pub enum WriterError {
    /// A filesystem operation on the output failed.
    #[error(transparent)]
    Io(#[from] IoError),

    /// Serializing an entity's JSON to bytes failed.
    #[error("An entity of type '{entity_type}' could not be serialized")]
    SimdSerialization {
        /// The serialization failure reported by the SIMD JSON writer.
        #[source]
        source: sonic_rs::Error,
        /// The entity type that could not be serialized.
        entity_type: NameBuf,
    },

    /// The HTTP client could not be initialized.
    #[error("The HTTP client could not be initialized")]
    ClientInit(#[from] reqwest::Error),

    /// A request to the broker failed at the transport layer.
    #[error("Broker request failed for {url}")]
    BrokerRequest {
        /// The transport failure reported by the HTTP layer.
        #[source]
        source: reqwest::Error,
        /// The endpoint the request targeted.
        url: Url,
    },

    /// The broker rejected a batch with a non-success status and explained why.
    #[error("Broker rejected a batch of {count} entities with status {status}: {reason}")]
    BrokerRejectedBatch {
        /// The HTTP status code returned.
        status: u16,
        /// How many entities were in the rejected batch.
        count: usize,
        /// The broker's own explanation.
        reason: Box<str>,
    },

    /// A batch was rejected with a body that is not RFC 7807 problem details, so the response
    /// explains nothing, typically an error page from a proxy in front of the broker.
    #[error("Broker returned status {status} with no problem details for a batch of {count} entities")]
    BrokerOpaqueStatus {
        /// The HTTP status code returned.
        status: u16,
        /// How many entities were in the rejected batch.
        count: usize,
    },

    /// A 207 Multi-Status arrived whose body could not be read, so which of its entities were
    /// written is unknown.
    #[error("Broker 207 Multi-Status body for {count} entities could not be read")]
    BrokerMultiStatusUnreadable {
        /// The parse failure.
        #[source]
        source: serde_json::Error,
        /// How many entities the batch carried.
        count: usize,
    },

    /// A 207 Multi-Status accounted for fewer entities than the batch carried.
    #[error("Broker accounted for {accounted} of {count} entities; {unaccounted} have no recorded outcome")]
    BrokerBatchUnaccounted {
        /// How many entities the batch carried.
        count: usize,
        /// How many the broker named in either list.
        accounted: usize,
        /// How many it never mentioned.
        unaccounted: usize,
    },

    /// A batch narrowed to a single entity still failed, so that entity was dropped.
    #[error("Entity {entity} was dropped after every delivery attempt failed")]
    BrokerEntityDropped {
        /// The entity that never reached the broker.
        entity: Urn,
    },

    /// Every delivery worker had exited before a batch could be handed to one.
    #[error("Broker worker pool is gone; {count} entities were dropped")]
    BrokerWorkerPoolGone {
        /// How many entities were dropped with the batch.
        count: usize,
    },

    /// The broker base URL was invalid.
    #[error("Invalid broker base URL {url}")]
    InvalidBrokerUrl {
        /// The parse failure reported by the URL parser.
        #[source]
        source: url::ParseError,
        /// The URL that could not be joined against.
        url: Url,
    },

    /// A broker worker thread panicked.
    #[error("Broker worker panicked: {message}")]
    BrokerWorkerPanicked {
        /// The panic payload, when it was a message the runtime could recover.
        message: Box<str>,
    },

    /// The run reached the broker but delivered nothing at all.
    #[error("Broker delivery failed: none of {failed} entities were written, for {distinct_reasons} distinct reason(s)")]
    BrokerDeliveryFailed {
        /// How many entities were dropped.
        failed: usize,
        /// How many distinct failures the run recorded.
        distinct_reasons: usize,
        /// The most frequent of them, kept whole so its own chain survives.
        #[source]
        cause: Box<WriterError>,
    },

    /// The atomic writer's spool could not be written, read, or (de)serialized.
    #[error(transparent)]
    AtomicSpool(#[from] AtomicSpoolError),
}

/// The result type used throughout the writing stage.
pub type Result<T> = result::Result<T, WriterError>;

#[cfg(test)]
mod tests {
    use crate::error::WriterError;
    use cassiopeia_common::error::io::{IoAction, IoError};
    use std::{error::Error, io, path::PathBuf};

    #[test]
    fn an_io_failure_is_wrapped_transparently() {
        let io = IoError::FileOperation {
            source: io::Error::other("boom"),
            path: PathBuf::from("/out/Sensor.json"),
            action: IoAction::Write,
        };

        assert!(WriterError::from(io).to_string().contains("Sensor.json"));
    }

    #[test]
    fn a_broker_rejection_names_the_status_the_batch_size_and_the_reason() {
        let error = WriterError::BrokerRejectedBatch {
            status: 422,
            count: 3,
            reason: "attribute 'dateObserved' is not a valid DateTime".into(),
        };
        let message = error.to_string();

        assert!(message.contains("422"));
        assert!(message.contains('3'));
        assert!(message.contains("dateObserved"));
    }

    #[test]
    fn a_delivery_failure_exposes_the_failure_it_was_built_from() {
        let error = WriterError::BrokerDeliveryFailed {
            failed: 100,
            distinct_reasons: 2,
            cause: Box::new(WriterError::BrokerOpaqueStatus { status: 502, count: 100 }),
        };

        assert!(error.to_string().contains("100"));
        assert!(error.source().expect("a source").to_string().contains("502"));
    }
}
