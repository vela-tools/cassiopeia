use crate::relationships::{InstanceRelationships, NestedRelationships, Relationships};
use cassiopeia_ngsi_ld::entity::name::NameBuf;
use getset::Getters;

/// Every relationship object minted for one entity, split by where each was recorded.
///
/// The three maps are filled from the same stored links and always travel together from assembly to
/// extraction; which map an object lands in is decided by the key it was minted under, never by
/// re-reading the source record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Getters)]
#[getset(get = "pub")]
pub struct EntityRelationships {
    /// Top-level relationship objects, keyed by attribute name.
    top_level: Relationships,
    /// Nested relationship objects, keyed by their path from the entity; `None` when the entity has
    /// none (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3).
    nested: Option<NestedRelationships>,
    /// Multi-attribute relationship objects, grouped per instance; `None` when the entity has none
    /// (clause 4.5.5).
    instances: Option<InstanceRelationships>,
}

/// The owned constituent parts of an [`EntityRelationships`], produced by
/// [`EntityRelationships::into_parts`].
pub type EntityRelationshipsParts = (Relationships, Option<NestedRelationships>, Option<InstanceRelationships>);

impl EntityRelationships {
    /// Gathers an entity's top-level, nested, and per-instance relationship objects.
    #[must_use]
    pub const fn new(top_level: Relationships, nested: Option<NestedRelationships>, instances: Option<InstanceRelationships>) -> EntityRelationships {
        EntityRelationships { top_level, nested, instances }
    }

    /// Whether the top-level attribute `name` carries at least one object, either directly or through
    /// one of its instances.
    #[must_use]
    pub fn has_objects(&self, name: &NameBuf) -> bool {
        self.top_level.get(name).is_some_and(|objects| !objects.is_empty())
            || self
                .instances
                .as_ref()
                .and_then(|instances| instances.get(name))
                .is_some_and(|per_instance| per_instance.values().any(|objects| !objects.is_empty()))
    }

    /// Drops every object recorded directly under the top-level attribute `name` or under one of its
    /// instances.
    pub fn remove_attribute(&mut self, name: &NameBuf) {
        self.top_level.shift_remove(name);
        if let Some(instances) = &mut self.instances {
            instances.shift_remove(name);
        }
    }

    /// Deconstructs the relationships into their owned maps, consuming them.
    #[must_use]
    pub fn into_parts(self) -> EntityRelationshipsParts {
        (self.top_level, self.nested, self.instances)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_relationships::EntityRelationships,
        instance_index::InstanceIndex,
        relationships::{InstanceObjects, InstanceRelationships, Relationships},
    };
    use cassiopeia_ngsi_ld::entity::name::NameBuf;
    use urn_rs::Urn;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    fn urn(value: &str) -> Urn {
        value.parse().expect("valid urn")
    }

    fn with_instance_objects() -> EntityRelationships {
        let mut top_level = Relationships::default();
        top_level.insert(name("refRoad"), vec![urn("urn:ngsi-ld:Road:1")]);
        let mut instances = InstanceRelationships::default();
        instances.insert(
            name("servesAirports"),
            InstanceObjects::from([(InstanceIndex::from(1), vec![urn("urn:ngsi-ld:Airport:340")])]),
        );
        EntityRelationships::new(top_level, None, Some(instances))
    }

    #[test]
    fn an_attribute_has_objects_whether_recorded_directly_or_per_instance() {
        let relationships = with_instance_objects();

        assert!(relationships.has_objects(&name("refRoad")));
        assert!(relationships.has_objects(&name("servesAirports")));
        assert!(!relationships.has_objects(&name("operatedBy")));
    }

    #[test]
    fn an_attribute_with_only_empty_object_lists_has_no_objects() {
        let mut top_level = Relationships::default();
        top_level.insert(name("refRoad"), Vec::new());
        let relationships = EntityRelationships::new(top_level, None, None);

        assert!(!relationships.has_objects(&name("refRoad")));
    }

    #[test]
    fn removing_an_attribute_drops_its_direct_and_its_instance_objects() {
        let mut relationships = with_instance_objects();

        relationships.remove_attribute(&name("servesAirports"));
        relationships.remove_attribute(&name("refRoad"));

        assert!(!relationships.has_objects(&name("servesAirports")));
        assert!(!relationships.has_objects(&name("refRoad")));
    }

    #[test]
    fn into_parts_returns_every_map() {
        let (top_level, nested, instances) = with_instance_objects().into_parts();

        assert_eq!(top_level.len(), 1);
        assert!(nested.is_none());
        assert_eq!(instances.map(|instances| instances.len()), Some(1));
    }
}
