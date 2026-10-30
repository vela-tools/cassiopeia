use crate::{
    entity::{Entity, EntityParts},
    entity_relationships::EntityRelationships,
};
use cassiopeia_mapping::mapping::Mapping;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
use chrono::{DateTime, Utc};
use getset::Getters;
use serde_json::Value as JsonValue;
use smallvec::{SmallVec, smallvec};
use std::sync::Arc;
use urn_rs::Urn;

/// One source record paired with the mapping that renders it into attributes.
///
/// Each fragment resolves through its own mapping against its own record: a current-state join of a
/// static geometry mapping and a temporal state mapping keeps each mapping's record distinct rather
/// than merging them, so two mappings that happen to read the same column do not collide.
pub type AssembledFragment = (JsonValue, Arc<Mapping>);

/// The fragments contributing to one emit-unit. One in the common case; several when more than one
/// mapping targets a single entity id (the current-state join). The inline capacity of one keeps the
/// single-fragment case off the heap.
pub type AssembledFragments = SmallVec<[AssembledFragment; 1]>;

/// The owned constituent parts of an [`AssembledEntity`], produced by [`AssembledEntity::into_parts`].
///
/// Each field is the [`AssembledEntity`] field of the same name.
#[derive(Debug)]
pub struct AssembledEntityParts {
    /// The entity's base URN, shared by every fragment.
    pub id: Urn,
    /// The entity's scope, when one was resolved.
    pub scope: Option<NgsiLdScope>,
    /// The entity's `expiresAt`, when one was resolved (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
    pub expires_at: Option<DateTime<Utc>>,
    /// Every relationship object minted for the entity: top-level, nested, and per instance.
    pub relationships: EntityRelationships,
    /// One `(record, mapping)` pair per fragment. Never empty.
    pub fragments: AssembledFragments,
}

/// One entity id's fragments, assembled from the store and ready for extraction.
///
/// Temporality now lives on attributes (ETSI GS CIM 009 v1.9.1 clause 4.5.5), so the resolver keys
/// its store by the base entity id and every mapping and every observation of that id merges into one
/// assembly. Each emit-unit the store returns becomes one `AssembledEntity`: a current-state unit
/// carries every mapping's fragment for the id, and a series unit carries one observation's fragment.
/// The relationships are read once and attach to every unit of the id.
#[derive(Debug, Clone, Getters)]
#[getset(get = "pub")]
pub struct AssembledEntity {
    /// The entity's base URN, shared by every fragment.
    id: Urn,
    /// The entity's scope, when one was resolved.
    scope: Option<NgsiLdScope>,
    /// The entity's `expiresAt`, when one was resolved (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
    expires_at: Option<DateTime<Utc>>,
    /// Every relationship object minted for the entity: top-level, nested, and per instance.
    relationships: EntityRelationships,
    /// One `(record, mapping)` pair per fragment, each rendered through its own mapping against its
    /// own record. Never empty.
    fragments: AssembledFragments,
}

impl AssembledEntity {
    /// Assembles an entity from its id, scope, entity `expiresAt`, relationships, and one or more
    /// fragments.
    #[must_use]
    pub const fn new(
        id: Urn,
        scope: Option<NgsiLdScope>,
        expires_at: Option<DateTime<Utc>>,
        relationships: EntityRelationships,
        fragments: AssembledFragments,
    ) -> AssembledEntity {
        AssembledEntity {
            id,
            scope,
            expires_at,
            relationships,
            fragments,
        }
    }

    /// Assembles a single-fragment entity from an [`Entity`] and the mapping that produced it.
    ///
    /// The entity's already-resolved values and metadata are discarded: an assembled entity holds only
    /// what extraction needs, namely the source record, its mapping, and the relationship objects.
    #[must_use]
    pub fn from_single(entity: Entity, mapping: Arc<Mapping>) -> AssembledEntity {
        let EntityParts {
            id,
            data,
            scope,
            expires_at,
            relationships,
            instance_relationships,
            nested_relationships,
            ..
        } = entity.into_parts();
        AssembledEntity {
            id,
            scope,
            expires_at,
            relationships: EntityRelationships::new(relationships, nested_relationships, instance_relationships),
            fragments: smallvec![(data, mapping)],
        }
    }

    /// Deconstructs the assembled entity into its owned parts, consuming it.
    #[must_use]
    pub fn into_parts(self) -> AssembledEntityParts {
        AssembledEntityParts {
            id: self.id,
            scope: self.scope,
            expires_at: self.expires_at,
            relationships: self.relationships,
            fragments: self.fragments,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        assembled_entity::{AssembledEntity, AssembledEntityParts},
        entity::Entity,
        instance_index::InstanceIndex,
        relationships::{InstanceObjects, InstanceRelationships},
    };
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use chrono::{TimeZone, Utc};
    use indexmap::IndexMap;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::{path::Path, sync::Arc};
    use urn_rs::Urn;

    fn mapping() -> Arc<Mapping> {
        let document = r#"{ version: "v4", dataModel: "Sensor", identity: { entityName: "S-{{ id }}" }, attributes: { v: { source: "{{ v }}" } } }"#;
        let mut runner = TemplateRunner::new();
        Arc::new(Mapping::from_json5(document, Path::new("test.json5"), &mut runner).unwrap())
    }

    fn urn() -> Urn {
        "urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap()
    }

    #[test]
    fn from_single_carries_the_record_and_its_mapping() {
        let entity = Entity::new(urn(), json!({"v": 1}), None, None, IndexMap::default(), None);
        let assembled = AssembledEntity::from_single(entity, mapping());

        assert_eq!(assembled.id(), &urn());
        assert_eq!(assembled.fragments().len(), 1);
        assert_eq!(assembled.fragments()[0].0, json!({"v": 1}));
    }

    #[test]
    fn from_single_carries_the_entity_instance_relationships() {
        let mut instances = InstanceRelationships::default();
        instances.insert(
            NameBuf::new("observedBy").unwrap(),
            InstanceObjects::from([(InstanceIndex::from(0), vec![urn()])]),
        );
        let mut entity = Entity::new(urn(), json!({"v": 1}), None, None, IndexMap::default(), None);
        entity.set_instance_relationships(Some(instances.clone()));

        let assembled = AssembledEntity::from_single(entity, mapping());

        assert_eq!(assembled.relationships().instances().as_ref(), Some(&instances));
    }

    #[test]
    fn from_single_carries_the_entity_expiry() {
        let expires_at = Utc.with_ymd_and_hms(2026, 10, 25, 13, 0, 0).unwrap();
        let entity = Entity::new(urn(), json!({"v": 1}), None, Some(expires_at), IndexMap::default(), None);
        let assembled = AssembledEntity::from_single(entity, mapping());

        assert_eq!(assembled.expires_at(), &Some(expires_at));
    }

    #[test]
    fn into_parts_returns_every_field() {
        let entity = Entity::new(urn(), json!({"v": 1}), None, None, IndexMap::default(), None);
        let assembled = AssembledEntity::from_single(entity, mapping());

        let AssembledEntityParts {
            id,
            scope,
            expires_at,
            relationships,
            fragments,
        } = assembled.into_parts();
        assert_eq!(id, urn());
        assert_eq!(scope, None);
        assert_eq!(expires_at, None);
        assert!(relationships.top_level().is_empty());
        assert_eq!(relationships.nested(), &None);
        assert_eq!(relationships.instances(), &None);
        assert_eq!(fragments.len(), 1);
    }
}
