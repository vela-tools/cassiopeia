use strum::{Display, EnumCount, EnumIter};

/// Why a delivery to an NGSI-LD Context Broker did not fully succeed.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum BrokerCode {
    /// The broker named one entity of a batch as rejected in a 207 Multi-Status body (ETSI GS CIM
    /// 009 v1.9.1 clause 5.2.17).
    EntityRejected,
    /// The broker rejected a whole batch with a status it explained in an RFC 7807 body.
    BatchRejected,
    /// The broker (or something in front of it) rejected a whole batch with a body that is not
    /// RFC 7807 problem details, so there is nothing typed to report but the status.
    BatchRejectedOpaque,
    /// A 207 Multi-Status arrived whose body could not be parsed, so which entities were written is
    /// unknown.
    BatchUnreadable,
    /// A 207 Multi-Status accounted for fewer entities than the batch carried, so some entities have
    /// no recorded outcome.
    BatchUnaccounted,
    /// The broker refused the payload as too large, so the batch was split and retried.
    PayloadTooLarge,
    /// A retryable status kept recurring until the retry budget ran out.
    RetriesExhausted,
    /// The request never reached the broker, and retrying did not help.
    TransportFailed,
    /// A batch narrowed to a single entity still failed, so that entity was dropped.
    EntityDropped,
    /// A batch could not be serialized into a request body.
    SerializationFailed,
    /// Every delivery worker had exited before a batch could be handed to one.
    WorkerPoolGone,
    /// A delivery worker thread panicked.
    WorkerPanicked,
    /// The run reached the broker but delivered nothing.
    DeliveryFailed,
    /// The atomic writer's spool could not be created, written, or replayed.
    SpoolFailed,
    /// The configured tenant is not a legal HTTP header value, so the tenant header was omitted.
    TenantHeaderInvalid,
}

#[cfg(test)]
mod tests {
    use crate::code::broker_code::BrokerCode;

    #[test]
    fn a_broker_code_renders_a_kebab_case_token() {
        assert_eq!(BrokerCode::EntityRejected.to_string(), "entity-rejected");
        assert_eq!(BrokerCode::PayloadTooLarge.to_string(), "payload-too-large");
    }
}
