use crate::{
    metadata::EntityMetadata,
    relationships::{InstanceRelationships, NestedRelationships, Relationships},
};
use cassiopeia_ngsi_ld::{
    entity::{name::NameBuf, scope::NgsiLdScope},
    value::types::Value as NgsiValue,
};
use foldhash::fast::RandomState;
use getset::{Getters, MutGetters, Setters};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use urn_rs::Urn;

/// An entity's resolved attribute values, keyed by attribute name in declaration order.
///
/// The keys are attribute names a mapping declared, and every attribute of every record is inserted
/// and then removed here, so the map hashes with `foldhash` rather than the standard library's
/// `SipHash`.
pub type AttributeValues = IndexMap<NameBuf, NgsiValue, RandomState>;

/// The owned constituent parts of an [`Entity`], produced by [`Entity::into_parts`].
pub type EntityParts = (
    Urn,
    JsonValue,
    Option<NgsiLdScope>,
    Relationships,
    Option<AttributeValues>,
    Option<EntityMetadata>,
    Option<InstanceRelationships>,
    Option<NestedRelationships>,
);

/// The final intermediate representation of an entity, ready for NGSI-LD serialization once resolved.
#[derive(Debug, Clone, Serialize, Deserialize, Getters, MutGetters, Setters)]
#[getset(get = "pub", set = "pub")]
pub struct Entity {
    /// The unique URN of the entity.
    id: Urn,
    /// The data payload.
    data: JsonValue,
    /// The scope of the entity.
    scope: Option<NgsiLdScope>,
    /// Relationships mapped by their attribute name.
    ///
    /// Mutable access exists so the extractor can drain a list relationship's flat objects while
    /// regrouping them into [`instance_relationships`](Self::instance_relationships).
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    relationships: Relationships,
    /// High-level values mapped by their attribute name.
    values: Option<AttributeValues>,
    /// Metadata storage for attributes, keyed by attribute name.
    metadata: Option<EntityMetadata>,
    /// Per-instance object lists for a `ListRelationship` that carries several `datasetId`-tagged
    /// instances (ETSI GS CIM 009 v1.9.1 clause 4.5.5), keyed by attribute name.
    ///
    /// One inner `Vec<Urn>` per surviving instance, in declaration order; `None` for the common case
    /// of an entity with no instance list relationships, so a plain entity allocates no map.
    instance_relationships: Option<InstanceRelationships>,
    /// Objects of relationships declared as sub-attributes, keyed by the [`RelationshipPath`] from the
    /// entity to the relationship (a nested relationship, ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with
    /// 4.5.3).
    ///
    /// Every key here is a multi-segment path; single-segment (top-level) relationships stay in
    /// [`relationships`](Self::relationships). `None` for the common case of an entity with no nested
    /// relationships, so a plain entity allocates no map.
    nested_relationships: Option<NestedRelationships>,
}

impl Entity {
    /// Builds an entity from its resolved identity, data, scope, relationships, and values.
    ///
    /// Metadata starts empty and is attached later by the extractor stage.
    #[must_use]
    pub const fn new(id: Urn, data: JsonValue, scope: Option<NgsiLdScope>, relationships: Relationships, values: Option<AttributeValues>) -> Entity {
        Entity {
            id,
            data,
            scope,
            relationships,
            values,
            metadata: None,
            instance_relationships: None,
            nested_relationships: None,
        }
    }

    /// Deconstructs the entity into its owned constituent parts, consuming it.
    #[must_use]
    pub fn into_parts(self) -> EntityParts {
        (
            self.id,
            self.data,
            self.scope,
            self.relationships,
            self.values,
            self.metadata,
            self.instance_relationships,
            self.nested_relationships,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity::Entity, relationship_path::RelationshipPath};
    use cassiopeia_ngsi_ld::{entity::name::NameBuf, value::types::Value as NgsiValue};
    use indexmap::IndexMap;
    use serde_json::json;
    use urn_rs::Urn;

    fn urn() -> Urn {
        "urn:ngsi-ld:Device:1".parse::<Urn>().expect("valid urn")
    }

    #[test]
    fn new_leaves_metadata_empty_and_preserves_the_other_parts() {
        let mut relationships = IndexMap::default();
        relationships.insert(NameBuf::new("refRoad").expect("valid"), vec![urn()]);
        let entity = Entity::new(urn(), json!({"id": "1"}), None, relationships.clone(), None);
        assert!(entity.metadata().is_none());
        assert_eq!(entity.relationships(), &relationships);
        let (id, _data, scope, rels, values, metadata, instance_relationships, nested_relationships) = entity.into_parts();
        assert_eq!(id, urn());
        assert!(scope.is_none());
        assert_eq!(rels, relationships);
        assert!(values.is_none());
        assert!(metadata.is_none());
        assert!(instance_relationships.is_none());
        assert!(nested_relationships.is_none());
    }

    #[test]
    fn set_values_and_metadata_are_reflected_by_the_getters() {
        let mut entity = Entity::new(urn(), json!({}), None, IndexMap::default(), None);
        let mut values = IndexMap::default();
        values.insert(NameBuf::new("temperature").expect("valid"), NgsiValue::Boolean(true));
        entity.set_values(Some(values.clone()));
        assert_eq!(entity.values().as_ref(), Some(&values));
    }

    #[test]
    fn nested_relationships_default_to_none_and_are_reflected_once_set() {
        let mut entity = Entity::new(urn(), json!({}), None, IndexMap::default(), None);
        assert!(entity.nested_relationships().is_none());

        let mut nested = IndexMap::default();
        nested.insert(
            RelationshipPath::from_segments(vec![NameBuf::new("directedBy").expect("valid"), NameBuf::new("playsCharacter").expect("valid")]),
            vec![urn()],
        );
        entity.set_nested_relationships(Some(nested.clone()));
        assert_eq!(entity.nested_relationships().as_ref(), Some(&nested));
    }
}
