use crate::entity::{
    NgsiLdEntity,
    attribute::{Attributes, NgsiLdAttributeWrapper},
    context::NgsiLdContext,
    name::NameBuf,
    scope::NgsiLdScope,
};
use urn_rs::Urn;

pub mod attribute;

/// Builds an [`NgsiLdEntity`] one field at a time.
pub struct NgsiLdEntityBuilder {
    id: Urn,
    entity_type: NameBuf,
    context: Option<NgsiLdContext>,
    scope: Option<NgsiLdScope>,
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

    #[test]
    fn the_builder_assembles_an_entity_with_its_id_and_type() {
        let entity = NgsiLdEntityBuilder::new("urn:ngsi-ld:Sensor:1".parse().unwrap(), NameBuf::new("Sensor").unwrap()).build();
        assert_eq!(entity.entity_type.as_str(), "Sensor");
        assert!(entity.attributes.is_empty());
    }
}
