use crate::{
    broker::{delivery_report::writer_diagnostic, request_shape::RequestShape, sender::SenderContext},
    error::WriterError,
};
use bytes::Bytes;
use cassiopeia_diagnostic::{code::broker_code::BrokerCode, context_field::ContextField, severity::Severity};
use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, representation::ReprAdapter};
use std::sync::atomic::Ordering;

/// Serializes a batch into the reusable scratch buffer, returning the payload bytes.
///
/// [`RequestShape::Array`] wraps the entities in a JSON array (the batch endpoints' body);
/// [`RequestShape::PerEntity`] serializes the single entity as a bare object, since the temporal
/// endpoint rejects arrays and a per-entity batch always holds exactly one entity. Concatenating
/// several bare objects would emit invalid JSON, so the one-entity invariant is enforced rather than
/// trusted.
///
/// Returns `None` when an entity cannot be serialized: serialization is deterministic, so the batch
/// is reported and counted as failed rather than retried.
pub fn serialize_batch(ctx: &SenderContext, entities: &[NgsiLdEntity], scratch: &mut Vec<u8>) -> Option<Bytes> {
    scratch.clear();
    match ctx.serialization.shape {
        RequestShape::Array => {
            scratch.push(b'[');
            for (index, entity) in entities.iter().enumerate() {
                if index > 0 {
                    scratch.push(b',');
                }
                if !serialize_entity_into(ctx, entity, entities.len(), scratch) {
                    return None;
                }
            }
            scratch.push(b']');
        }
        RequestShape::PerEntity => {
            // The per-entity endpoints take exactly one entity; the writer pins a per-entity batch to
            // a single entity, so more than one here would corrupt the body.
            debug_assert_eq!(entities.len(), 1, "a per-entity batch must hold exactly one entity");
            let entity = entities.first()?;
            if !serialize_entity_into(ctx, entity, 1, scratch) {
                return None;
            }
        }
    }

    Some(Bytes::copy_from_slice(scratch))
}

/// Serializes one entity's representation into `scratch`, returning `false` after reporting and
/// counting the whole batch as failed when serialization fails.
///
/// The entity streams straight into `sonic-rs` through a [`ReprAdapter`], with no intermediate
/// `serde_json::Value` tree.
fn serialize_entity_into(ctx: &SenderContext, entity: &NgsiLdEntity, batch_len: usize, scratch: &mut Vec<u8>) -> bool {
    let adapter = ReprAdapter::new(entity, ctx.serialization.representation, ctx.serialization.skip_null);
    if let Err(source) = sonic_rs::to_writer(&mut *scratch, &adapter) {
        let error = WriterError::SimdSerialization {
            source,
            entity_type: entity.entity_type.clone(),
        };
        let diagnostic = writer_diagnostic(
            Severity::Error,
            BrokerCode::SerializationFailed,
            format!("A batch of {batch_len} entities could not be serialized for the broker"),
            &error,
            vec![ContextField::EntityType(entity.entity_type.clone())],
        );
        ctx.counters.failures.record(BrokerCode::SerializationFailed, error);
        ctx.runtime.reporter.report(&diagnostic);
        ctx.counters.failed.fetch_add(batch_len, Ordering::Relaxed);
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use crate::broker::{
        request_shape::RequestShape,
        sender::test_support::{context, thing},
        serialize::serialize_batch,
    };

    #[test]
    fn an_array_shape_serializes_a_json_array() {
        let ctx = context(RequestShape::Array);
        let mut scratch = Vec::new();

        let payload = serialize_batch(&ctx, &[thing("urn:ngsi-ld:Thing:1"), thing("urn:ngsi-ld:Thing:2")], &mut scratch).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();

        assert_eq!(parsed.as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_per_entity_shape_serializes_a_bare_object() {
        let ctx = context(RequestShape::PerEntity);
        let mut scratch = Vec::new();

        let payload = serialize_batch(&ctx, &[thing("urn:ngsi-ld:Thing:1")], &mut scratch).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();

        assert!(parsed.is_object());
        assert_eq!(parsed["id"], "urn:ngsi-ld:Thing:1");
    }
}
