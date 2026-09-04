use crate::broker::{
    batch_job::BatchJob,
    batch_settlement::report_entity_dropped,
    broker_failures::BrokerFailures,
    metrics::BrokerMetrics,
    post::{SendOutcome, post_batch},
    request_shape::RequestShape,
    serialize::serialize_batch,
};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, context::ContextSource};
use cassiopeia_reporter::reporter::Reporter;
use crossbeam_channel::Receiver;
use reqwest::{
    blocking::Client,
    header::{HeaderName, HeaderValue},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use url::Url;

/// The request-building inputs a worker uses to construct and address each POST.
///
/// Every field is cheap to clone: the reqwest `Client` shares one connection pool across its clones,
/// the endpoint is a shared `Arc<Url>`, and the headers are `HeaderValue`s and a shared `Arc` slice.
///
/// The endpoint stays a parsed [`Url`] rather than its text: reqwest re-parses a string on every
/// request, so keeping it parsed removes a full URL parse per POST, and a diagnostic that names the
/// endpoint gets a typed value rather than another string to interpret.
#[derive(Clone)]
pub struct RequestParts {
    /// The HTTP client, sharing one connection pool across workers.
    pub client: Client,
    /// The fully-resolved endpoint URL, with any operation query already baked in.
    pub endpoint: Arc<Url>,
    /// The `Content-Type` header for every request.
    pub content_type: HeaderValue,
    /// The `Link` header, when the context is delivered out of band.
    pub link_header: Option<HeaderValue>,
    /// The `NGSILD-Tenant` header, when a tenant is configured.
    pub tenant_header: Option<HeaderValue>,
    /// The extra request headers (typically credentials), pre-marked sensitive; shared across workers.
    pub auth_headers: Arc<[(HeaderName, HeaderValue)]>,
}

/// The serialization inputs a worker uses to turn entities into a request body.
#[derive(Clone)]
pub struct SerializationParts {
    /// The NGSI-LD representation entities serialize in.
    pub representation: NgsiLdRepresentation,
    /// Whether null-valued attributes are skipped.
    pub skip_null: NgsiLdSkipNull,
    /// The shared context source for body injection, when in body mode.
    pub body_context: Option<Arc<ContextSource>>,
    /// The initial per-worker serialization scratch capacity, in bytes.
    pub initial_payload_bytes: usize,
    /// Whether the request body is a JSON array of entities or a single entity object.
    pub shape: RequestShape,
}

/// The shared counters and congestion metrics workers update as they run.
#[derive(Clone)]
pub struct SharedCounters {
    /// The shared count of successfully written entities.
    pub written: Arc<AtomicUsize>,
    /// The shared count of dropped entities.
    pub failed: Arc<AtomicUsize>,
    /// The shared congestion metrics.
    pub metrics: Arc<BrokerMetrics>,
    /// The distinct delivery failures the run has recorded, shared across the worker pool.
    pub failures: Arc<BrokerFailures>,
}

/// The runtime controls a worker observes: retry policy, shutdown, and reporting.
#[derive(Clone)]
pub struct RuntimeParts {
    /// The retry ceiling before a batch is split or dropped.
    pub max_retries: u32,
    /// The process-wide shutdown flag.
    pub shutdown: &'static AtomicBool,
    /// The reporter for warnings and errors.
    pub reporter: &'static dyn Reporter,
}

/// The immutable per-worker context a sender thread runs against.
///
/// One template is built in `BrokerWriter::new` and cloned per worker; each sub-bundle groups the
/// fields one concern owns so a function takes only the parts it needs.
#[derive(Clone)]
pub struct SenderContext {
    /// How to build and address each request.
    pub request: RequestParts,
    /// How to serialize entities into a request body.
    pub serialization: SerializationParts,
    /// The shared counters and metrics.
    pub counters: SharedCounters,
    /// The retry, shutdown, and reporting controls.
    pub runtime: RuntimeParts,
}

/// The worker loop: drains batches off the channel, POSTs each, and splits-and-retries on failure.
///
/// Serialization is deterministic, so a serialization failure drops the batch (splitting cannot
/// help). A transport error, a 5xx, a 429, or a 413 falls through to the split path so entities stay
/// alive; a non-retryable 4xx (a schema or auth bug) drops the batch without splitting. A 207
/// Multi-Status is a per-entity data outcome, not a size or congestion signal, so its failures are
/// reported and counted rather than split-retried.
pub fn sender_loop(receiver: &Receiver<BatchJob>, ctx: &SenderContext) {
    // Per-worker scratch, grown once to the largest batch the worker sees, then steady-state zero.
    let mut scratch: Vec<u8> = Vec::with_capacity(ctx.serialization.initial_payload_bytes);
    // Reusable split-retry stack, avoiding allocation on the common no-split path.
    let mut stack: Vec<Vec<NgsiLdEntity>> = Vec::new();

    while let Ok(mut job) = receiver.recv() {
        if ctx.runtime.shutdown.load(Ordering::Acquire) {
            ctx.counters.failed.fetch_add(job.entities.len(), Ordering::Relaxed);
            break;
        }

        // Body-inject the `@context` once per job, before any splitting. Only the small context
        // returned by `resolve()` is cloned onto each entity; the source map itself is shared.
        if let Some(source) = &ctx.serialization.body_context {
            for entity in &mut job.entities {
                if let Some(context) = source.resolve(&entity.entity_type) {
                    entity.context = Some(context.clone());
                }
            }
        }

        stack.push(job.entities);
        while let Some(mut entities) = stack.pop() {
            let entity_count = entities.len();
            if entity_count == 0 {
                continue;
            }
            if ctx.runtime.shutdown.load(Ordering::Acquire) {
                ctx.counters.failed.fetch_add(entity_count, Ordering::Relaxed);
                continue;
            }

            let Some(payload) = serialize_batch(ctx, &entities, &mut scratch) else {
                continue;
            };
            ctx.counters.metrics.record_entity_size(payload.len(), entity_count);

            match post_batch(ctx, &payload, &entities) {
                SendOutcome::Done => {}
                SendOutcome::Split => {
                    if entity_count > 1 {
                        let tail = entities.split_off(entity_count / 2);
                        // LIFO: push the tail first so the head runs next.
                        stack.push(tail);
                        stack.push(entities);
                    } else if let Some(entity) = entities.first() {
                        report_entity_dropped(ctx, entity);
                        ctx.counters.failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::broker::{
        broker_failures::BrokerFailures,
        metrics::BrokerMetrics,
        request_shape::RequestShape,
        sender::{RequestParts, RuntimeParts, SenderContext, SerializationParts, SharedCounters},
        test_reporter::{static_shutdown, static_test_reporter},
    };
    use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use reqwest::{blocking::Client, header::HeaderValue};
    use std::sync::{Arc, atomic::AtomicUsize};
    use url::Url;
    use urn_rs::Urn;

    /// Builds a sender context wired to a throwaway client and the static test reporter.
    pub(crate) fn context(shape: RequestShape) -> SenderContext {
        SenderContext {
            request: RequestParts {
                client: Client::builder().build().unwrap(),
                endpoint: Arc::new(Url::parse("https://b/ngsi-ld/v1/entityOperations/upsert").unwrap()),
                content_type: HeaderValue::from_static("application/json"),
                link_header: None,
                tenant_header: None,
                auth_headers: Vec::new().into(),
            },
            serialization: SerializationParts {
                representation: NgsiLdRepresentation::Normalized,
                skip_null: NgsiLdSkipNull::Skip,
                body_context: None,
                initial_payload_bytes: 1024,
                shape,
            },
            counters: SharedCounters {
                written: Arc::new(AtomicUsize::new(0)),
                failed: Arc::new(AtomicUsize::new(0)),
                metrics: Arc::new(BrokerMetrics::default()),
                failures: Arc::new(BrokerFailures::new()),
            },
            runtime: RuntimeParts {
                max_retries: 3,
                shutdown: static_shutdown(),
                reporter: static_test_reporter(),
            },
        }
    }

    /// Builds a minimal entity with the given id and a fixed type.
    pub(crate) fn thing(id: &str) -> NgsiLdEntity {
        NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new("Thing").unwrap())
    }
}
