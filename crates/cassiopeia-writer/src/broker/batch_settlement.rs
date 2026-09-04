//! What the writer records and reports for one interpreted broker response.
//!
//! Split out from the attempt loop because the two change for different reasons: the loop owns when
//! a request is retried, split, or given up on, while this owns what each outcome costs the run's
//! counters and what it says in the report.

use crate::{
    broker::{
        backoff::{BackoffSleep, backoff_delay, sleep_or_shutdown},
        batch_reconciliation::reconcile,
        batch_result::{BatchEntityError, BatchOperationResult},
        broker_rejection::BrokerRejection,
        delivery_report::{DeliveryContext, delivery_diagnostic, rejection_fields},
        post::{BODY_ECHO_CAP, SendOutcome},
        sender::SenderContext,
    },
    error::WriterError,
};
use cassiopeia_common::captured_body::CapturedBody;
use cassiopeia_diagnostic::{code::broker_code::BrokerCode, context_field::ContextField, severity::Severity};
use cassiopeia_ngsi_ld::entity::NgsiLdEntity;
use http::StatusCode;
use std::{slice::from_ref, sync::atomic::Ordering};

/// Whether the attempt loop goes round again, or is finished with the batch.
pub enum RetryStep {
    /// Try the request again.
    Again,
    /// The batch is settled.
    Settled(SendOutcome),
}

/// Accounts for a 207 Multi-Status: reconciles it against the batch, reports every named rejection,
/// and reports any entity the body never mentioned.
pub fn settle_partial(ctx: &SenderContext, delivery: &DeliveryContext<'_>, result: &BatchOperationResult, status: StatusCode) -> SendOutcome {
    let entity_count = delivery.entities.len();
    let outcome = reconcile(result, delivery.entities);
    ctx.counters.written.fetch_add(outcome.written(), Ordering::Relaxed);
    ctx.counters.failed.fetch_add(outcome.failed(), Ordering::Relaxed);
    for error in &result.errors {
        report_entity_rejection(ctx, error, delivery);
    }
    if !outcome.unaccounted().is_empty() {
        let accounted = entity_count.saturating_sub(outcome.unaccounted().len());
        fail(
            ctx,
            BrokerCode::BatchUnaccounted,
            Severity::Error,
            format!("Broker accounted for {accounted} of {entity_count} entities"),
            WriterError::BrokerBatchUnaccounted {
                count: entity_count,
                accounted,
                unaccounted: outcome.unaccounted().len(),
            },
            delivery,
            vec![ContextField::HttpStatus(status)],
        );
    }
    // Per-entity data errors, not a size or congestion signal: nothing to retry.
    SendOutcome::Done
}

/// Accounts for a 207 whose body could not be read: nothing in it can be trusted as written.
pub fn settle_unreadable(
    ctx: &SenderContext,
    delivery: &DeliveryContext<'_>,
    source: serde_json::Error,
    body: &CapturedBody,
    status: StatusCode,
) -> SendOutcome {
    let entity_count = delivery.entities.len();
    ctx.counters.failed.fetch_add(entity_count, Ordering::Relaxed);
    fail(
        ctx,
        BrokerCode::BatchUnreadable,
        Severity::Error,
        format!("Broker 207 Multi-Status body could not be read; {entity_count} entities have no recorded outcome"),
        WriterError::BrokerMultiStatusUnreadable { source, count: entity_count },
        delivery,
        vec![ContextField::HttpStatus(status), ContextField::BodyEcho(body.echo(BODY_ECHO_CAP))],
    );
    SendOutcome::Done
}

/// Accounts for a refused payload: remembers the size the broker would not take, and splits.
pub fn settle_too_large(
    ctx: &SenderContext,
    delivery: &DeliveryContext<'_>,
    problem: Option<&BrokerRejection>,
    status: StatusCode,
    payload_len: usize,
) -> SendOutcome {
    let entity_count = delivery.entities.len();
    ctx.counters.metrics.record_failure();
    observe_payload_limit(ctx, payload_len);
    fail(
        ctx,
        BrokerCode::PayloadTooLarge,
        Severity::Warning,
        format!("Broker refused a {payload_len} byte payload of {entity_count} entities; splitting"),
        WriterError::BrokerRejectedBatch {
            status: status.as_u16(),
            count: entity_count,
            reason: problem.map_or("payload too large", BrokerRejection::text).into(),
        },
        delivery,
        problem_context(status, payload_len, problem),
    );
    SendOutcome::Split
}

/// Accounts for a batch the broker refused and explained.
pub fn settle_rejected(ctx: &SenderContext, delivery: &DeliveryContext<'_>, status: StatusCode, problem: &BrokerRejection) -> SendOutcome {
    let entity_count = delivery.entities.len();
    ctx.counters.failed.fetch_add(entity_count, Ordering::Relaxed);
    let mut extra = rejection_fields(problem);
    extra.push(ContextField::HttpStatus(status));
    fail(
        ctx,
        BrokerCode::BatchRejected,
        Severity::Error,
        format!("Broker rejected {entity_count} entities"),
        WriterError::BrokerRejectedBatch {
            status: status.as_u16(),
            count: entity_count,
            reason: problem.text().into(),
        },
        delivery,
        extra,
    );
    SendOutcome::Done
}

/// Accounts for a batch refused with a body that explains nothing, echoing what did arrive.
pub fn settle_opaque(ctx: &SenderContext, delivery: &DeliveryContext<'_>, status: StatusCode, body: &CapturedBody) -> SendOutcome {
    let entity_count = delivery.entities.len();
    ctx.counters.failed.fetch_add(entity_count, Ordering::Relaxed);
    fail(
        ctx,
        BrokerCode::BatchRejectedOpaque,
        Severity::Error,
        format!("Broker rejected {entity_count} entities without problem details"),
        WriterError::BrokerOpaqueStatus {
            status: status.as_u16(),
            count: entity_count,
        },
        delivery,
        vec![ContextField::HttpStatus(status), ContextField::BodyEcho(body.echo(BODY_ECHO_CAP))],
    );
    SendOutcome::Done
}

/// Counts one retryable answer and either waits for the next attempt or gives up on this batch size.
///
/// Nothing is reported until the retry budget is spent: a broker under sustained load answers 503 for
/// every attempt, and reporting each one would flood the diagnostics with repeats of the same failure.
pub fn on_retryable(
    ctx: &SenderContext,
    delivery: &DeliveryContext<'_>,
    status: StatusCode,
    problem: Option<&BrokerRejection>,
    payload_len: usize,
    attempt: &mut u32,
) -> RetryStep {
    let entity_count = delivery.entities.len();
    ctx.counters.metrics.record_failure();
    *attempt += 1;
    ctx.counters.metrics.retries.fetch_add(1, Ordering::Relaxed);

    if *attempt >= ctx.runtime.max_retries {
        let mut extra = problem_context(status, payload_len, problem);
        extra.push(attempt_field(*attempt, ctx.runtime.max_retries));
        fail(
            ctx,
            BrokerCode::RetriesExhausted,
            Severity::Warning,
            format!("Broker kept answering {} after {attempt} attempts; splitting the batch", status.as_u16()),
            WriterError::BrokerRejectedBatch {
                status: status.as_u16(),
                count: entity_count,
                reason: problem.map_or("no problem details", BrokerRejection::text).into(),
            },
            delivery,
            extra,
        );
        return RetryStep::Settled(SendOutcome::Split);
    }

    let delay = backoff_delay(*attempt);
    ctx.counters
        .metrics
        .backoff_ns
        .fetch_add(u64::try_from(delay.as_nanos()).unwrap_or(u64::MAX), Ordering::Relaxed);
    if sleep_or_shutdown(delay, ctx.runtime.shutdown) == BackoffSleep::ShutdownRequested {
        ctx.counters.failed.fetch_add(entity_count, Ordering::Relaxed);
        return RetryStep::Settled(SendOutcome::Done);
    }
    RetryStep::Again
}

/// Counts one transport failure and either waits for the next attempt or gives up on this batch size.
///
/// Splitting on exhaustion lets the caller retry smaller batches when the failure was caused by
/// request size or broker capacity rather than by the network.
pub fn on_transport_error(ctx: &SenderContext, delivery: &DeliveryContext<'_>, source: reqwest::Error, attempt: &mut u32) -> RetryStep {
    let entity_count = delivery.entities.len();
    ctx.counters.metrics.record_failure();
    *attempt += 1;
    ctx.counters.metrics.retries.fetch_add(1, Ordering::Relaxed);

    if *attempt >= ctx.runtime.max_retries {
        let error = WriterError::BrokerRequest {
            source,
            url: ctx.request.endpoint.as_ref().clone(),
        };
        fail(
            ctx,
            BrokerCode::TransportFailed,
            Severity::Warning,
            format!("Broker request failed after {attempt} attempts; splitting the batch"),
            error,
            delivery,
            vec![attempt_field(*attempt, ctx.runtime.max_retries)],
        );
        return RetryStep::Settled(SendOutcome::Split);
    }

    if sleep_or_shutdown(backoff_delay(*attempt), ctx.runtime.shutdown) == BackoffSleep::ShutdownRequested {
        ctx.counters.failed.fetch_add(entity_count, Ordering::Relaxed);
        return RetryStep::Settled(SendOutcome::Done);
    }
    RetryStep::Again
}

/// Records a delivery failure against the run and reports it once.
fn fail(
    ctx: &SenderContext,
    code: BrokerCode,
    severity: Severity,
    headline: String,
    error: WriterError,
    delivery: &DeliveryContext<'_>,
    extra: Vec<ContextField>,
) {
    let diagnostic = delivery_diagnostic(severity, code, headline, &error, delivery, extra);
    ctx.counters.failures.record(code, error);
    ctx.runtime.reporter.report(&diagnostic);
}

/// Reports one 207 per-entity rejection, naming the entity and the broker's own explanation.
fn report_entity_rejection(ctx: &SenderContext, error: &BatchEntityError, delivery: &DeliveryContext<'_>) {
    let rejection = BrokerRejection::new(error.error.clone());
    let mut extra = rejection_fields(&rejection);
    extra.push(ContextField::Entities {
        first: error.entity_id.clone(),
        additional: 0,
    });
    if let Some(status) = error.error.status.and_then(|status| StatusCode::from_u16(status).ok()) {
        extra.push(ContextField::HttpStatus(status));
    }
    if let Some(registration) = &error.registration_id {
        extra.push(ContextField::RegistrationId(registration.clone()));
    }

    let recorded = WriterError::BrokerRejectedBatch {
        status: error.error.status.unwrap_or_else(|| StatusCode::MULTI_STATUS.as_u16()),
        count: 1,
        reason: rejection.text().into(),
    };
    let diagnostic = delivery_diagnostic(
        Severity::Error,
        BrokerCode::EntityRejected,
        format!("Broker rejected entity {}", error.entity_id),
        &recorded,
        delivery,
        extra,
    );
    ctx.counters.failures.record(BrokerCode::EntityRejected, recorded);
    ctx.runtime.reporter.report(&diagnostic);
}

/// Reports the entity a single-entity batch dropped after every attempt failed.
pub fn report_entity_dropped(ctx: &SenderContext, entity: &NgsiLdEntity) {
    let error = WriterError::BrokerEntityDropped { entity: entity.id.clone() };
    let diagnostic = delivery_diagnostic(
        Severity::Error,
        BrokerCode::EntityDropped,
        format!("Entity {} was dropped after every delivery attempt failed", entity.id),
        &error,
        &DeliveryContext {
            endpoint: &ctx.request.endpoint,
            entities: from_ref(entity),
        },
        Vec::new(),
    );
    ctx.counters.failures.record(BrokerCode::EntityDropped, error);
    ctx.runtime.reporter.report(&diagnostic);
}

/// The fields a problem body and a request size contribute to a diagnostic.
fn problem_context(status: StatusCode, payload_len: usize, problem: Option<&BrokerRejection>) -> Vec<ContextField> {
    let mut fields = match problem {
        Some(rejection) => rejection_fields(rejection),
        None => Vec::with_capacity(2),
    };
    fields.push(ContextField::HttpStatus(status));
    fields.push(ContextField::PayloadBytes(u64::try_from(payload_len).unwrap_or(u64::MAX)));
    fields
}

/// Which attempt of how many this was.
fn attempt_field(attempt: u32, limit: u32) -> ContextField {
    let attempt_number = u64::from(attempt);
    let limit_number = u64::from(limit);
    ContextField::Attempt {
        attempt: attempt_number,
        limit: limit_number,
    }
}

/// Remembers the smallest payload size the broker has refused, so the sizing controller backs off.
fn observe_payload_limit(ctx: &SenderContext, payload_len: usize) {
    let previous = ctx.counters.metrics.observed_payload_limit.load(Ordering::Relaxed);
    if previous == 0 || previous > payload_len {
        ctx.counters.metrics.observed_payload_limit.store(payload_len, Ordering::Relaxed);
    }
}
