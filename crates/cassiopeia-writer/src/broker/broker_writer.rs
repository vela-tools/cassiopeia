use crate::{
    broker::{
        batch_job::BatchJob,
        broker_context_config::BrokerContextConfig,
        broker_failures::BrokerFailures,
        broker_identity::BrokerIdentity,
        broker_runtime::BrokerRuntime,
        broker_serialization::BrokerSerialization,
        broker_transport::BrokerTransport,
        client_builder::{ClientSettings, build_client},
        config::BrokerWriterConfig,
        context_mode::resolve_context_mode,
        controller::adjust_target_bytes,
        delivery_report::writer_diagnostic,
        metrics::BrokerMetrics,
        request_headers::{sensitive_auth_headers, tenant_header},
        request_shape::{RequestShape, route},
        sender::{RequestParts, RuntimeParts, SenderContext, SerializationParts, SharedCounters, sender_loop},
        tuning::BrokerTuning,
    },
    error::{Result, WriterError},
    run_outcome::RunOutcome,
    writer::{Writer, WriterProgress, WriterStats},
};
use cassiopeia_diagnostic::{code::broker_code::BrokerCode, context_field::ContextField, severity::Severity};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use cassiopeia_reporter::{reporter::Reporter, stage_id::StageId};
use crossbeam_channel::{Receiver, Sender, bounded};
use std::{
    any::Any,
    mem,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use url::Url;

/// The batch size an array-shaped run starts at, before any payload has been observed. A per-entity
/// operation is pinned to one entity per request instead.
const INITIAL_BATCH_SIZE: usize = 100;

/// A streaming writer that POSTs batches of entities to an NGSI-LD Context Broker.
///
/// The main thread accumulates entities, serializes a batch, and ships the bytes into a bounded
/// channel; N worker threads share the receiver and run blocking HTTP upserts. An AIMD controller
/// watches worker-emitted latency and failure metrics and nudges the target payload size each flush,
/// and the entity count per batch is derived from it, so both payload size and batch size track
/// broker health bidirectionally throughout the run.
pub struct BrokerWriter {
    front_buffer: Vec<NgsiLdEntity>,
    current_batch_size: usize,
    current_target_bytes: usize,
    request_shape: RequestShape,
    tuning: BrokerTuning,
    metrics: Arc<BrokerMetrics>,
    sender: Option<Sender<BatchJob>>,
    written_count: Arc<AtomicUsize>,
    failed_count: Arc<AtomicUsize>,
    failures: Arc<BrokerFailures>,
    handles: Vec<thread::JoinHandle<()>>,
    stage_id: Option<StageId>,
    reporter: &'static dyn Reporter,
}

impl BrokerWriter {
    /// Builds the writer, its HTTP client, and its pool of sender threads.
    ///
    /// # Errors
    /// Returns a [`WriterError`] when the operation's endpoint URL is invalid or the HTTP client
    /// cannot be built.
    pub fn new(config: BrokerWriterConfig) -> Result<BrokerWriter> {
        let BrokerWriterConfig {
            transport,
            serialization,
            context: context_config,
            identity,
            runtime,
            operation,
        } = config;
        let BrokerTransport {
            base_url,
            user_agent,
            timeout,
            pool_max_idle_per_host,
            tcp_keepalive,
            sender_threads,
            channel_capacity,
            max_retries,
            http2_prior_knowledge,
            tuning,
        } = transport;
        let BrokerSerialization { representation, skip_null } = serialization;
        let BrokerContextConfig {
            source: context_source,
            delivery: context_delivery,
        } = context_config;
        let BrokerIdentity { tenant, headers } = identity;
        let BrokerRuntime { stage_id, shutdown, reporter } = runtime;

        let routing = route(operation);
        let endpoint = resolve_endpoint(&base_url, routing.path, routing.query)?;

        let client = build_client(ClientSettings {
            timeout,
            pool_max_idle_per_host,
            tcp_keepalive,
            user_agent,
            http2_prior_knowledge,
        })?;
        let context_mode = resolve_context_mode(context_delivery, context_source, reporter);

        let written_count = Arc::new(AtomicUsize::new(0));
        let failed_count = Arc::new(AtomicUsize::new(0));
        let failures = Arc::new(BrokerFailures::new());
        let metrics = Arc::new(BrokerMetrics::default());

        let (sender, receiver) = bounded::<BatchJob>(channel_capacity.max(1));

        let context = SenderContext {
            request: RequestParts {
                client,
                endpoint,
                content_type: context_mode.content_type(),
                link_header: context_mode.link_header(),
                tenant_header: tenant_header(tenant.as_ref(), reporter),
                auth_headers: sensitive_auth_headers(&headers),
            },
            serialization: SerializationParts {
                representation,
                skip_null,
                body_context: context_mode.body_context(),
                initial_payload_bytes: tuning.initial_payload_bytes,
                shape: routing.shape,
            },
            counters: SharedCounters {
                written: Arc::clone(&written_count),
                failed: Arc::clone(&failed_count),
                metrics: Arc::clone(&metrics),
                failures: Arc::clone(&failures),
            },
            runtime: RuntimeParts {
                max_retries,
                shutdown,
                reporter,
            },
        };

        let handles = spawn_worker_pool(&context, sender_threads.max(1), receiver);

        let initial_target = tuning.initial_payload_bytes.clamp(tuning.min_payload_bytes, tuning.max_payload_bytes);

        // A per-entity operation posts exactly one entity per request, so the batch is pinned to one
        // and the adaptive sizer never runs; only an array operation grows the batch.
        let initial_batch_size = match routing.shape {
            RequestShape::Array => INITIAL_BATCH_SIZE,
            RequestShape::PerEntity => 1,
        };

        Ok(BrokerWriter {
            front_buffer: Vec::with_capacity(initial_batch_size),
            current_batch_size: initial_batch_size,
            current_target_bytes: initial_target,
            request_shape: routing.shape,
            tuning,
            metrics,
            sender: Some(sender),
            written_count,
            failed_count,
            failures,
            handles,
            stage_id,
            reporter,
        })
    }

    /// Ships the accumulated front buffer to the worker pool as one batch, re-sizing the next batch
    /// from the latest congestion signal.
    fn flush_to_sender(&mut self) {
        if self.front_buffer.is_empty() {
            return;
        }

        let batch_len = self.front_buffer.len();
        let entities = mem::replace(&mut self.front_buffer, Vec::with_capacity(self.current_batch_size));

        // AIMD sizing governs array batches only; a per-entity operation stays pinned to one entity.
        if self.request_shape == RequestShape::Array {
            self.current_target_bytes = adjust_target_bytes(self.current_target_bytes, &self.metrics, &self.tuning);
            self.update_batch_size();
        }

        // Publish a live latency readout so the terminal writer-stage row reflects broker health.
        if let Some(stage_id) = self.stage_id {
            let latency = self.metrics.avg_latency_ms.load(Ordering::Relaxed);
            if latency > 0 {
                self.reporter.stage_set_message(stage_id, &format!("~{latency}ms"));
            }
        }

        if let Some(sender) = &self.sender {
            let started = Instant::now();
            let result = sender.send(BatchJob { entities });
            self.metrics
                .queue_wait_ns
                .fetch_add(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX), Ordering::Relaxed);
            if result.is_err() {
                // Every worker receiver has dropped: count the batch as failed rather than lose it.
                self.failed_count.fetch_add(batch_len, Ordering::Relaxed);
                let error = WriterError::BrokerWorkerPoolGone { count: batch_len };
                let diagnostic = writer_diagnostic(
                    Severity::Error,
                    BrokerCode::WorkerPoolGone,
                    format!("Every broker delivery worker has exited; {batch_len} entities were dropped"),
                    &error,
                    vec![ContextField::BatchSize(u64::try_from(batch_len).unwrap_or(u64::MAX))],
                );
                self.failures.record(BrokerCode::WorkerPoolGone, error);
                self.reporter.report(&diagnostic);
            }
        }
    }

    /// Derives the next batch's entity count from the target payload size and observed entity size.
    fn update_batch_size(&mut self) {
        let avg = self.metrics.avg_entity_size_bytes.load(Ordering::Relaxed);
        if let Some(optimal) = self.current_target_bytes.checked_div(avg) {
            self.current_batch_size = optimal.clamp(self.tuning.min_batch_size, self.tuning.max_batch_size);
        }
    }
}

impl Writer for BrokerWriter {
    fn write(&mut self, entity: NgsiLdEntity) -> Result<()> {
        self.front_buffer.push(entity);
        if self.front_buffer.len() >= self.current_batch_size {
            self.flush_to_sender();
        }
        Ok(())
    }

    fn progress(&self) -> WriterProgress {
        WriterProgress {
            written: self.written_count.load(Ordering::Acquire),
            failed: self.failed_count.load(Ordering::Acquire),
            bytes_written: self.metrics.bytes_delivered.load(Ordering::Acquire),
        }
    }

    fn finalize(&mut self, _outcome: RunOutcome) -> Result<WriterStats> {
        self.flush_to_sender();

        // Dropping the last main-thread sender closes the channel; each worker exits on its next
        // `recv()`.
        self.sender.take();

        for handle in self.handles.drain(..) {
            if let Err(payload) = handle.join() {
                let error = WriterError::BrokerWorkerPanicked {
                    message: panic_message(payload.as_ref()),
                };
                let diagnostic = writer_diagnostic(
                    Severity::Error,
                    BrokerCode::WorkerPanicked,
                    "A broker delivery worker panicked".to_owned(),
                    &error,
                    Vec::new(),
                );
                self.failures.record(BrokerCode::WorkerPanicked, error);
                self.reporter.report(&diagnostic);
            }
        }

        let written = self.written_count.load(Ordering::Acquire);
        let failed = self.failed_count.load(Ordering::Acquire);

        // Partial progress is not total failure: entities that did land are still delivered, and a
        // run that lost some of a batch reports that through its counters. A run that delivered
        // nothing at all is a different thing, and must fail the pipeline rather than exit clean.
        if written == 0
            && failed > 0
            && let Some(summary) = self.failures.drain()
        {
            return Err(WriterError::BrokerDeliveryFailed {
                failed,
                distinct_reasons: summary.distinct,
                cause: Box::new(summary.most_frequent),
            });
        }

        Ok(WriterStats {
            written,
            failed,
            bytes_written: self.metrics.bytes_delivered.load(Ordering::Acquire),
            request_time: Duration::from_nanos(self.metrics.request_time_ns.load(Ordering::Acquire)),
            retries: self.metrics.retries.load(Ordering::Acquire),
            backoff: Duration::from_nanos(self.metrics.backoff_ns.load(Ordering::Acquire)),
            queue_wait: Duration::from_nanos(self.metrics.queue_wait_ns.load(Ordering::Acquire)),
        })
    }
}

impl Drop for BrokerWriter {
    fn drop(&mut self) {
        self.sender.take();
        // A no-op when `finalize` already drained; explicit so the double-join contract is clear.
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

/// Bakes the operation's path and query into the endpoint every worker posts to, so workers stay
/// query-agnostic and reqwest never re-parses a URL per request.
///
/// # Errors
/// Returns [`WriterError::InvalidBrokerUrl`] when the operation's path cannot be joined onto the
/// configured base URL.
fn resolve_endpoint(base_url: &Url, path: &str, query: Option<&str>) -> Result<Arc<Url>> {
    let mut endpoint = base_url.join(path).map_err(|source| WriterError::InvalidBrokerUrl {
        source,
        // The error owns the URL after the borrowed configuration is dropped.
        url: base_url.clone(),
    })?;
    endpoint.set_query(query);
    Ok(Arc::new(endpoint))
}

/// Starts the delivery workers, each with its own clone of the shared context.
///
/// The master receiver is consumed here rather than borrowed: dropping it is what closes the channel
/// once every worker's clone is gone, so the ownership is the shutdown mechanism.
fn spawn_worker_pool(context: &SenderContext, threads: usize, receiver: Receiver<BatchJob>) -> Vec<thread::JoinHandle<()>> {
    let mut handles = Vec::with_capacity(threads);
    for _ in 0..threads {
        let receiver = receiver.clone();
        let context = context.clone();
        handles.push(thread::spawn(move || sender_loop(&receiver, &context)));
    }
    drop(receiver);
    handles
}

/// Recovers a panic payload's message, which the standard library boxes as a `&str` or a `String`
/// depending on how the panic was raised. Discarding it would throw away the only description of
/// what went wrong inside the worker.
fn panic_message(payload: &(dyn Any + Send)) -> Box<str> {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).into();
    }
    match payload.downcast_ref::<String>() {
        Some(message) => message.as_str().into(),
        None => "the panic payload was not a message".into(),
    }
}

#[cfg(test)]
mod tests {
    use crate::broker::{
        batch_job::BatchJob,
        broker_failures::BrokerFailures,
        broker_writer::{BrokerWriter, INITIAL_BATCH_SIZE},
        metrics::BrokerMetrics,
        request_shape::RequestShape,
        test_reporter::TestReporter,
        tuning::BrokerTuning,
    };
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use cassiopeia_reporter::stage_id::StageId;
    use crossbeam_channel::bounded;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
    };
    use urn_rs::Urn;

    fn thing() -> NgsiLdEntity {
        NgsiLdEntity::new("urn:ngsi-ld:Thing:1".parse::<Urn>().unwrap(), NameBuf::new("Thing").unwrap())
    }

    #[test]
    fn a_flush_publishes_the_latency_readout_when_a_stage_id_is_set() {
        static REPORTER: TestReporter = TestReporter::new();

        // A dummy worker drains the channel so `send()` does not block.
        let (sender, receiver) = bounded::<BatchJob>(8);
        let handle = thread::spawn(move || while receiver.recv().is_ok() {});

        let metrics = Arc::new(BrokerMetrics::default());
        metrics.avg_latency_ms.store(123, Ordering::Relaxed);

        let tuning = BrokerTuning::default();
        let mut writer = BrokerWriter {
            front_buffer: vec![thing()],
            current_batch_size: INITIAL_BATCH_SIZE,
            current_target_bytes: tuning.initial_payload_bytes,
            request_shape: RequestShape::Array,
            tuning,
            metrics,
            sender: Some(sender),
            written_count: Arc::new(AtomicUsize::new(0)),
            failed_count: Arc::new(AtomicUsize::new(0)),
            failures: Arc::new(BrokerFailures::new()),
            handles: vec![handle],
            stage_id: Some(StageId::new("writer")),
            reporter: &REPORTER,
        };

        writer.flush_to_sender();
        writer.sender.take();
        for handle in writer.handles.drain(..) {
            let _ = handle.join();
        }

        assert!(REPORTER.messages().iter().any(|(id, message)| id == "writer" && message == "~123ms"));
    }

    #[test]
    fn a_flush_skips_the_latency_readout_when_no_stage_id_is_set() {
        static REPORTER: TestReporter = TestReporter::new();

        let (sender, receiver) = bounded::<BatchJob>(8);
        let handle = thread::spawn(move || while receiver.recv().is_ok() {});

        let metrics = Arc::new(BrokerMetrics::default());
        metrics.avg_latency_ms.store(999, Ordering::Relaxed);

        let tuning = BrokerTuning::default();
        let mut writer = BrokerWriter {
            front_buffer: vec![thing()],
            current_batch_size: INITIAL_BATCH_SIZE,
            current_target_bytes: tuning.initial_payload_bytes,
            request_shape: RequestShape::Array,
            tuning,
            metrics,
            sender: Some(sender),
            written_count: Arc::new(AtomicUsize::new(0)),
            failed_count: Arc::new(AtomicUsize::new(0)),
            failures: Arc::new(BrokerFailures::new()),
            handles: vec![handle],
            stage_id: None,
            reporter: &REPORTER,
        };

        writer.flush_to_sender();
        writer.sender.take();
        for handle in writer.handles.drain(..) {
            let _ = handle.join();
        }

        assert!(REPORTER.messages().is_empty());
    }
}

#[cfg(test)]
mod finalize_tests {
    use crate::{
        broker::{
            broker_failures::BrokerFailures,
            broker_writer::{BrokerWriter, panic_message},
            metrics::BrokerMetrics,
            request_shape::RequestShape,
            test_reporter::{TestReporter, static_test_reporter},
            tuning::BrokerTuning,
        },
        error::WriterError,
        run_outcome::RunOutcome,
        writer::Writer,
    };
    use cassiopeia_diagnostic::code::broker_code::BrokerCode;
    use std::{
        any::Any,
        error::Error,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    /// A writer whose worker pool has already finished, with the given tallies and recorded failures.
    fn settled(written: usize, failed: usize, failures: Arc<BrokerFailures>, reporter: &'static TestReporter) -> BrokerWriter {
        let tuning = BrokerTuning::default();
        BrokerWriter {
            front_buffer: Vec::new(),
            current_batch_size: 1,
            current_target_bytes: tuning.initial_payload_bytes,
            request_shape: RequestShape::Array,
            tuning,
            metrics: Arc::new(BrokerMetrics::default()),
            sender: None,
            written_count: Arc::new(AtomicUsize::new(written)),
            failed_count: Arc::new(AtomicUsize::new(failed)),
            failures,
            handles: Vec::new(),
            stage_id: None,
            reporter,
        }
    }

    fn recorded(status: u16) -> Arc<BrokerFailures> {
        let failures = Arc::new(BrokerFailures::new());
        failures.record(BrokerCode::BatchRejectedOpaque, WriterError::BrokerOpaqueStatus { status, count: 10 });
        failures.record(BrokerCode::BatchRejectedOpaque, WriterError::BrokerOpaqueStatus { status, count: 10 });
        failures.record(BrokerCode::TransportFailed, WriterError::BrokerWorkerPoolGone { count: 1 });
        failures
    }

    #[test]
    fn a_run_that_delivered_nothing_finalizes_with_a_typed_delivery_failure() {
        let mut writer = settled(0, 20, recorded(502), static_test_reporter());

        let Err(WriterError::BrokerDeliveryFailed {
            failed,
            distinct_reasons,
            cause,
        }) = writer.finalize(RunOutcome::Committed)
        else {
            panic!("a run that delivered nothing must fail the pipeline");
        };

        assert_eq!(failed, 20);
        assert_eq!(distinct_reasons, 2);
        // The most frequent recorded failure is the one the report chains to.
        assert!(cause.to_string().contains("502"));
    }

    #[test]
    fn a_delivery_failure_exposes_its_cause_through_the_source_chain() {
        let mut writer = settled(0, 20, recorded(502), static_test_reporter());

        let error = writer.finalize(RunOutcome::Committed).expect_err("a delivery failure");

        assert!(error.source().expect("a chained cause").to_string().contains("502"));
    }

    #[test]
    fn partial_delivery_still_finalizes_cleanly() {
        let mut writer = settled(3, 2, recorded(502), static_test_reporter());

        let stats = writer.finalize(RunOutcome::Committed).expect("partial progress is not total failure");

        assert_eq!(stats.written, 3);
        assert_eq!(stats.failed, 2);
    }

    #[test]
    fn a_run_that_delivered_nothing_and_recorded_nothing_finalizes_cleanly() {
        // Nothing to deliver is not a delivery failure: an empty run writes nothing and fails nothing.
        let mut writer = settled(0, 0, Arc::new(BrokerFailures::new()), static_test_reporter());

        assert!(writer.finalize(RunOutcome::Committed).is_ok());
    }

    #[test]
    fn a_panicking_worker_carries_its_message() {
        let from_str: Box<dyn Any + Send> = Box::new("the sender loop gave up");
        let from_string: Box<dyn Any + Send> = Box::new("formatted panic".to_owned());
        let opaque: Box<dyn Any + Send> = Box::new(7_u32);

        assert_eq!(panic_message(from_str.as_ref()).as_ref(), "the sender loop gave up");
        assert_eq!(panic_message(from_string.as_ref()).as_ref(), "formatted panic");
        assert!(panic_message(opaque.as_ref()).contains("not a message"));
    }

    #[test]
    fn a_written_count_is_still_read_after_a_failed_finalize() {
        let failures = recorded(502);
        let mut writer = settled(0, 5, Arc::clone(&failures), static_test_reporter());

        let _ = writer.finalize(RunOutcome::Committed);

        // Draining the record is what the failure did; a second finalize has nothing left to blame.
        assert!(failures.is_empty());
        assert_eq!(writer.written_count.load(Ordering::Acquire), 0);
    }
}
