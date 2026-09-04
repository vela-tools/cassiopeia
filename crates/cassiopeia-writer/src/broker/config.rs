use crate::{
    broker::{
        broker_context_config::BrokerContextConfig,
        broker_identity::BrokerIdentity,
        broker_runtime::BrokerRuntime,
        broker_serialization::BrokerSerialization,
        broker_transport::BrokerTransport,
        http2_prior_knowledge::Http2PriorKnowledge,
        tuning::BrokerTuning,
    },
    context_delivery::ContextDelivery,
};
use cassiopeia_common::{
    broker_header::BrokerHeaders,
    broker_operation::BrokerOperation,
    representation::NgsiLdRepresentation,
    skip_null::NgsiLdSkipNull,
    tenant::Tenant,
};
use cassiopeia_ngsi_ld::entity::context::ContextSource;
use cassiopeia_reporter::{reporter::Reporter, stage_id::StageId};
use std::sync::atomic::AtomicBool;
use url::Url;

/// Configuration for a [`BrokerWriter`](crate::broker::broker_writer::BrokerWriter).
///
/// The settings are grouped into cohesive sub-bundles (transport, serialization, `@context`,
/// identity, and runtime) with the NGSI-LD operation standing on its own. `BrokerWriter::new`
/// destructures the bundles; the `with_*` builders reach into whichever one a setting belongs to.
pub struct BrokerWriterConfig {
    /// The HTTP transport and worker-pool settings.
    pub transport: BrokerTransport,
    /// How entities serialize onto the wire.
    pub serialization: BrokerSerialization,
    /// The `@context` and its delivery mode.
    pub context: BrokerContextConfig,
    /// The tenant and credential headers.
    pub identity: BrokerIdentity,
    /// The stage, shutdown, and reporting hooks.
    pub runtime: BrokerRuntime,
    /// Which NGSI-LD operation each request performs.
    pub operation: BrokerOperation,
}

impl BrokerWriterConfig {
    /// Builds a config for an already-parsed broker base URL.
    ///
    /// The URL arrives typed rather than as text: the manifest has already parsed it, and taking it
    /// back apart to re-parse here would lose that guarantee and turn a settled value into another
    /// failure path.
    #[must_use]
    pub fn new(base_url: Url, user_agent: String, shutdown: &'static AtomicBool, reporter: &'static dyn Reporter) -> BrokerWriterConfig {
        BrokerWriterConfig {
            transport: BrokerTransport::new(base_url, user_agent),
            serialization: BrokerSerialization::default(),
            context: BrokerContextConfig::new(),
            identity: BrokerIdentity::default(),
            runtime: BrokerRuntime::new(shutdown, reporter),
            operation: BrokerOperation::default(),
        }
    }

    /// Overrides the adaptive-controller tuning.
    #[must_use]
    pub const fn with_tuning(mut self, tuning: BrokerTuning) -> BrokerWriterConfig {
        self.transport.tuning = tuning;
        self
    }

    /// Overrides the serialization representation.
    #[must_use]
    pub const fn with_representation(mut self, representation: NgsiLdRepresentation) -> BrokerWriterConfig {
        self.serialization.representation = representation;
        self
    }

    /// Overrides whether null-valued attributes are skipped.
    #[must_use]
    pub const fn with_skip_null(mut self, skip_null: NgsiLdSkipNull) -> BrokerWriterConfig {
        self.serialization.skip_null = skip_null;
        self
    }

    /// Sets the `@context` source.
    #[must_use]
    pub fn with_context(mut self, context: ContextSource) -> BrokerWriterConfig {
        self.context.source = context;
        self
    }

    /// Sets how the `@context` is delivered.
    #[must_use]
    pub const fn with_context_delivery(mut self, context_delivery: ContextDelivery) -> BrokerWriterConfig {
        self.context.delivery = context_delivery;
        self
    }

    /// Sets which NGSI-LD operation each request performs.
    #[must_use]
    pub const fn with_operation(mut self, operation: BrokerOperation) -> BrokerWriterConfig {
        self.operation = operation;
        self
    }

    /// Sets the tenant header.
    #[must_use]
    pub fn with_tenant(mut self, tenant: Option<Tenant>) -> BrokerWriterConfig {
        self.identity.tenant = tenant;
        self
    }

    /// Sets the extra request headers attached to every broker request.
    #[must_use]
    pub fn with_headers(mut self, headers: BrokerHeaders) -> BrokerWriterConfig {
        self.identity.headers = headers;
        self
    }

    /// Sets whether HTTP/2 prior knowledge (h2c) is forced. Leave [`Http2PriorKnowledge::Off`] for
    /// HTTPS brokers.
    #[must_use]
    pub const fn with_http2_prior_knowledge(mut self, http2_prior_knowledge: Http2PriorKnowledge) -> BrokerWriterConfig {
        self.transport.http2_prior_knowledge = http2_prior_knowledge;
        self
    }

    /// Sets the reporter stage id for the live latency readout.
    #[must_use]
    pub const fn with_stage_id(mut self, stage_id: StageId) -> BrokerWriterConfig {
        self.runtime.stage_id = Some(stage_id);
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::broker::{
        config::BrokerWriterConfig,
        test_reporter::{static_shutdown, static_test_reporter},
    };
    use url::Url;

    #[test]
    fn the_config_keeps_the_base_url_it_was_given() {
        let config = BrokerWriterConfig::new(
            Url::parse("https://broker.example.com/").unwrap(),
            "ua".into(),
            static_shutdown(),
            static_test_reporter(),
        );

        assert_eq!(config.transport.base_url.as_str(), "https://broker.example.com/");
    }
}
