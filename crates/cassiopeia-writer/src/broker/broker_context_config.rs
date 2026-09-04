use crate::context_delivery::ContextDelivery;
use cassiopeia_ngsi_ld::entity::context::ContextSource;

/// The `@context` a broker writer attaches to entities and how it is delivered.
pub struct BrokerContextConfig {
    /// The `@context` source to attach to entities.
    pub source: ContextSource,
    /// How the `@context` is delivered (embedded in the body vs referenced by a `Link` header).
    pub delivery: ContextDelivery,
}

impl BrokerContextConfig {
    /// Builds a context configuration with no `@context` and body delivery, the defaults a broker
    /// writer starts from.
    #[must_use]
    pub fn new() -> BrokerContextConfig {
        BrokerContextConfig {
            source: ContextSource::None,
            delivery: ContextDelivery::default(),
        }
    }
}

impl Default for BrokerContextConfig {
    /// A manual impl is required because [`ContextSource`] has no `Default` to derive from; the
    /// default source is [`ContextSource::None`].
    fn default() -> BrokerContextConfig {
        BrokerContextConfig::new()
    }
}
