use crate::{
    entity::Entity,
    relationships::{NestedRelationships, Relationships},
};
use cassiopeia_mapping::mapping::Mapping;
use cassiopeia_ngsi_ld::entity::scope::NgsiLdScope;
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
pub type AssembledEntityParts = (Urn, Option<NgsiLdScope>, Relationships, Option<NestedRelationships>, AssembledFragments);

/// One entity id's fragments, assembled from the store and ready for extraction.
///
/// Temporality now lives on attributes (ETSI GS CIM 009 v1.9.1 clause 4.5.5), so the resolver keys
/// its store by the base entity id and every mapping and every observation of that id merges into one
/// assembly. Each emit-unit the store returns becomes one `AssembledEntity`: a current-state unit
/// carries every mapping's fragment for the id, and a series unit carries one observation's fragment.
/// The relationships are read once and attach to the id's first unit.
#[derive(Debug, Clone, Getters)]
#[getset(get = "pub")]
pub struct AssembledEntity {
    /// The entity's base URN, shared by every fragment.
    id: Urn,
    /// The entity's scope, when one was resolved.
    scope: Option<NgsiLdScope>,
    /// Top-level relationship targets, keyed by attribute name.
    relationships: Relationships,
    /// Nested relationship targets, keyed by their path from the entity; `None` when the entity has
    /// none (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3).
    nested_relationships: Option<NestedRelationships>,
    /// One `(record, mapping)` pair per fragment, each rendered through its own mapping against its
    /// own record. Never empty.
    fragments: AssembledFragments,
}

impl AssembledEntity {
    /// Assembles an entity from its id, scope, relationships, and one or more fragments.
    #[must_use]
    pub const fn new(
        id: Urn,
        scope: Option<NgsiLdScope>,
        relationships: Relationships,
        nested_relationships: Option<NestedRelationships>,
        fragments: AssembledFragments,
    ) -> AssembledEntity {
        AssembledEntity {
            id,
            scope,
            relationships,
            nested_relationships,
            fragments,
        }
    }

    /// Assembles a single-fragment entity from an [`Entity`] and the mapping that produced it.
    ///
    /// The entity's already-resolved value, metadata, and instance-relationship maps are discarded:
    /// an assembled entity holds only what extraction needs, namely the source record, its mapping,
    /// and the resolved relationships.
    #[must_use]
    pub fn from_single(entity: Entity, mapping: Arc<Mapping>) -> AssembledEntity {
        let (id, data, scope, relationships, _values, _metadata, _instance_relationships, nested_relationships) = entity.into_parts();
        AssembledEntity {
            id,
            scope,
            relationships,
            nested_relationships,
            fragments: smallvec![(data, mapping)],
        }
    }

    /// Deconstructs the assembled entity into its owned parts, consuming it.
    #[must_use]
    pub fn into_parts(self) -> AssembledEntityParts {
        (self.id, self.scope, self.relationships, self.nested_relationships, self.fragments)
    }
}

#[cfg(test)]
mod tests {
    use crate::{assembled_entity::AssembledEntity, entity::Entity};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use indexmap::IndexMap;
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
        let entity = Entity::new(urn(), json!({"v": 1}), None, IndexMap::default(), None);
        let assembled = AssembledEntity::from_single(entity, mapping());

        assert_eq!(assembled.id(), &urn());
        assert_eq!(assembled.fragments().len(), 1);
        assert_eq!(assembled.fragments()[0].0, json!({"v": 1}));
    }

    #[test]
    fn into_parts_returns_every_field() {
        let entity = Entity::new(urn(), json!({"v": 1}), None, IndexMap::default(), None);
        let assembled = AssembledEntity::from_single(entity, mapping());

        let (id, scope, relationships, nested, fragments) = assembled.into_parts();
        assert_eq!(id, urn());
        assert!(scope.is_none());
        assert!(relationships.is_empty());
        assert!(nested.is_none());
        assert_eq!(fragments.len(), 1);
    }
}
