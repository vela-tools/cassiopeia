use cassiopeia_mapping::{
    mapping::Mapping,
    template::{error::TemplateError, resolver::TemplateResolver},
};
use cassiopeia_ngsi_ld::value::types::Value as NgsiValue;
use cassiopeia_unreadable_timestamps::{unreadable_expiries::UnreadableExpiries, unreadable_timestamp::unreadable_text};
use chrono::{DateTime, Utc};
use serde_json::Value;
use urn_rs::Urn;

/// Resolves the entity `expiresAt` each expanded record declares through its mapping.
///
/// Like the [`UrnGenerator`](crate::urn::generator::UrnGenerator) it sits beside, it is stateless
/// apart from the resolver, which shares its templating engine through an `Arc`, so every pipeline
/// worker can hold its own clone without contending on a lock.
#[derive(Clone)]
pub struct ExpiryResolver {
    /// The shared resolver used to evaluate `expiresAt` templates.
    resolver: TemplateResolver,
}

impl ExpiryResolver {
    /// Creates an expiry resolver over a shared template resolver.
    #[must_use]
    pub const fn new(resolver: TemplateResolver) -> ExpiryResolver {
        ExpiryResolver { resolver }
    }

    /// Resolves the entity `expiresAt` for one record from its mapping's `expiresAt` declaration.
    ///
    /// A mapping without the declaration yields `None`, as does a template whose plain path reads a
    /// missing or empty field: a record that carries no expiry contributes none, exactly as it
    /// contributes no scope. Resolved text is read in the same spellings as attribute date-times,
    /// and a number as a Unix epoch. Text that reads as no instant costs the record its expiry but
    /// not the record itself; the loss is recorded in `unreadable` under the mapping's entity type,
    /// with `entity` as its exemplar, in parity with an attribute qualifier that would not read
    /// (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError`] when the template fails to resolve.
    pub fn resolve(&self, mapping: &Mapping, entity: &Urn, data: &Value, unreadable: &UnreadableExpiries) -> Result<Option<DateTime<Utc>>, TemplateError> {
        let Some(template) = mapping.compiled_expires_at() else {
            return Ok(None);
        };
        let Some(resolved) = self.resolver.resolve_complete(template, data)? else {
            return Ok(None);
        };

        let value = NgsiValue::from(resolved);
        let instant = value.try_parse_datetime();
        if instant.is_none()
            && let Some(text) = unreadable_text(&value)
        {
            unreadable.record(mapping.data_model().entity_type(), entity, text.as_ref());
        }

        Ok(instant)
    }
}
