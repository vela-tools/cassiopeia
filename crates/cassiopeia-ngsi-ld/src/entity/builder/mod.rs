use crate::entity::{
    NgsiLdEntity,
    attribute::{Attributes, NgsiLdAttributeWrapper},
    context::NgsiLdContext,
    name::NameBuf,
    scope::NgsiLdScope,
};
use chrono::{DateTime, Utc};
use urn_rs::Urn;

pub mod attribute;

/// Builds an [`NgsiLdEntity`] one field at a time.
pub struct NgsiLdEntityBuilder {
    id: Urn,
    entity_type: NameBuf,
    context: Option<NgsiLdContext>,
    scope: Option<NgsiLdScope>,
    expires_at: Option<DateTime<Utc>>,
    attributes: Attributes,
}

impl NgsiLdEntityBuilder {
    /// Starts a builder for an entity with the given id and type.
    #[must_use]
    pub fn new(id: Urn, entity_type: NameBuf) -> NgsiLdEntityBuilder {
        NgsiLdEntityBuilder {
            id,
            entity_type,
            context: None,
            scope: None,
            expires_at: None,
            attributes: Attributes::default(),
        }
    }

    /// Sets the entity's `@context`.
    #[must_use]
    pub fn context(mut self, context: NgsiLdContext) -> Self {
        self.context = Some(context);
        self
    }

    /// Sets the entity's scope.
    #[must_use]
    pub fn scope(mut self, scope: NgsiLdScope) -> Self {
        self.scope = Some(scope);
        self
    }

    /// Sets the instant after which the entity ceases to hold (ETSI GS CIM 009 v1.9.1 clause 4.22),
    /// written as an optional member of the entity (clause 4.5.1).
    #[must_use]
    pub const fn expires_at(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Adds one attribute under `name`.
    #[must_use]
    pub fn attribute(mut self, name: NameBuf, attribute: NgsiLdAttributeWrapper) -> Self {
        self.attributes.insert(name, attribute);
        self
    }

    /// Assembles the entity.
    ///
    /// Construction is infallible: the id ([`Urn`]) and type ([`NameBuf`]) are validated by their
    /// own types. Entity-wide structural validation is provided separately by the validation
    /// module.
    #[must_use]
    pub fn build(self) -> NgsiLdEntity {
        NgsiLdEntity {
            id: self.id,
            entity_type: self.entity_type,
            context: self.context,
            scope: self.scope,
            expires_at: self.expires_at,
            attributes: self.attributes,
        }
    }
}

impl NgsiLdEntity {
    /// Starts a builder for an entity with the given id and type.
    #[must_use]
    pub fn builder(id: Urn, entity_type: NameBuf) -> NgsiLdEntityBuilder {
        NgsiLdEntityBuilder::new(id, entity_type)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::{builder::NgsiLdEntityBuilder, name::NameBuf};
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_builder_assembles_an_entity_with_its_id_and_type() {
        let entity = NgsiLdEntityBuilder::new("urn:ngsi-ld:Sensor:1".parse().unwrap(), NameBuf::new("Sensor").unwrap()).build();
        assert_eq!(entity.entity_type.as_str(), "Sensor");
        assert_eq!(entity.expires_at, None);
        assert!(entity.attributes.is_empty());
    }

    #[test]
    fn the_builder_carries_a_set_expiry() {
        let expires_at = Utc.with_ymd_and_hms(2026, 10, 25, 13, 0, 0).unwrap();
        let entity = NgsiLdEntityBuilder::new("urn:ngsi-ld:Sensor:1".parse().unwrap(), NameBuf::new("Sensor").unwrap())
            .expires_at(expires_at)
            .build();

        assert_eq!(entity.expires_at, Some(expires_at));
    }
}
