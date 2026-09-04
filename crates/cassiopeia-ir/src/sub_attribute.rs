use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};
use foldhash::fast::RandomState;
use getset::Getters;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A set of resolved sub-attributes, keyed by name in declaration order.
///
/// The keys are sub-attribute names a mapping declared (trusted configuration rather than
/// attacker-controlled input), and every attribute of every record builds and probes one of these,
/// so it hashes with `foldhash` rather than the standard library's `SipHash`.
pub type SubAttributes = IndexMap<NameBuf, SubAttribute, RandomState>;

/// One sub-attribute resolved for an NGSI-LD attribute, carrying the kind it was declared as.
///
/// An attribute's sub-attributes are the serialization of a Property or any of its subclasses (ETSI
/// GS CIM 009 v1.9.1 clause 4.5.2.2), so a sub-attribute keeps its declared [`NgsiLdAttributeKind`]:
/// a nested `VocabProperty`, `ListProperty`, `LanguageProperty`, `GeoProperty`, or `JsonProperty` is
/// built as that kind rather than flattened to a plain Property. It carries its kind-shaped [`Value`]
/// payload and its own sub-attributes, nested to arbitrary depth.
///
/// A sub-attribute is single-instance: unlike a top-level attribute it has no `datasetId`-keyed
/// instances (ETSI GS CIM 009 v1.9.1 clause 4.5.5 admits multiple instances only on top-level
/// attributes), so its own sub-attributes are a flat map rather than a
/// [`MetadataStorage`](crate::metadata::MetadataStorage).
///
/// A sub-attribute may also be a Relationship or `ListRelationship` (a relationship carried by
/// another attribute, ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3): its `value` then holds the
/// object URN, or the array of object URNs, and `object_type` names the target entity type. For the
/// Property family `object_type` is `None`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Getters)]
#[getset(get = "pub")]
pub struct SubAttribute {
    /// The NGSI-LD attribute kind this sub-attribute was declared as.
    kind: NgsiLdAttributeKind,
    /// The kind-shaped value payload: a `VocabProperty`'s vocabulary IRI, a `LanguageProperty`'s
    /// `languageMap` object, a `ListProperty`'s array, a plain Property's scalar, a Relationship's
    /// object URN, a `ListRelationship`'s array of object URNs, and so on.
    value: Value,
    /// The target entity type of a Relationship/`ListRelationship` sub-attribute (its `objectType`,
    /// ETSI GS CIM 009 v1.9.1 clause 4.5.3); `None` for the Property family.
    object_type: Option<NameBuf>,
    /// This sub-attribute's own sub-attributes, keyed by name and nested to arbitrary depth.
    metadata: SubAttributes,
}

impl SubAttribute {
    /// Builds a Property-family sub-attribute from its declared kind, resolved value, and nested
    /// sub-attributes. The target type is `None`, since only a relationship carries one.
    #[must_use]
    pub const fn new(kind: NgsiLdAttributeKind, value: Value, metadata: SubAttributes) -> SubAttribute {
        SubAttribute {
            kind,
            value,
            object_type: None,
            metadata,
        }
    }

    /// Builds a relationship sub-attribute, carrying the target entity type its `objectType`
    /// serializes to (ETSI GS CIM 009 v1.9.1 clause 4.5.3).
    #[must_use]
    pub const fn new_relationship(kind: NgsiLdAttributeKind, value: Value, object_type: Option<NameBuf>, metadata: SubAttributes) -> SubAttribute {
        SubAttribute {
            kind,
            value,
            object_type,
            metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sub_attribute::SubAttribute;
    use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, name::NameBuf};
    use indexmap::IndexMap;
    use serde_json::json;

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).expect("valid name")
    }

    #[test]
    fn a_sub_attribute_exposes_its_kind_value_and_nested_sub_attributes() {
        let inner = SubAttribute::new(NgsiLdAttributeKind::Property, json!("2026-08-26T00:00:00Z"), IndexMap::default());
        let mut nested = IndexMap::default();
        nested.insert(name("observedAt"), inner);
        let outer = SubAttribute::new(NgsiLdAttributeKind::VocabProperty, json!("https://example.org/level/high"), nested);

        assert_eq!(*outer.kind(), NgsiLdAttributeKind::VocabProperty);
        assert_eq!(outer.value(), &json!("https://example.org/level/high"));
        assert_eq!(
            *outer.metadata().get(&name("observedAt")).expect("nested sub-attribute").kind(),
            NgsiLdAttributeKind::Property
        );
    }

    #[test]
    fn a_property_sub_attribute_has_no_object_type() {
        let sub = SubAttribute::new(NgsiLdAttributeKind::Property, json!(0.5), IndexMap::default());

        assert!(sub.object_type().is_none());
    }

    #[test]
    fn a_relationship_sub_attribute_carries_its_object_type() {
        let sub = SubAttribute::new_relationship(
            NgsiLdAttributeKind::Relationship,
            json!("urn:ngsi-ld:Character:JackSparrow"),
            Some(name("Character")),
            IndexMap::default(),
        );

        assert_eq!(*sub.kind(), NgsiLdAttributeKind::Relationship);
        assert_eq!(sub.object_type().as_ref().map(NameBuf::as_str), Some("Character"));
        assert_eq!(sub.value(), &json!("urn:ngsi-ld:Character:JackSparrow"));
    }

    #[test]
    fn a_sub_attribute_round_trips_through_json() {
        let sub = SubAttribute::new(NgsiLdAttributeKind::ListProperty, json!([1, 2, 3]), IndexMap::default());
        let encoded = serde_json::to_string(&sub).expect("serializes");
        let decoded: SubAttribute = serde_json::from_str(&encoded).expect("deserializes");

        assert_eq!(sub, decoded);
    }

    #[test]
    fn a_relationship_sub_attribute_round_trips_through_json() {
        let sub = SubAttribute::new_relationship(
            NgsiLdAttributeKind::ListRelationship,
            json!(["urn:ngsi-ld:Person:1", "urn:ngsi-ld:Person:2"]),
            Some(name("Person")),
            IndexMap::default(),
        );
        let encoded = serde_json::to_string(&sub).expect("serializes");
        let decoded: SubAttribute = serde_json::from_str(&encoded).expect("deserializes");

        assert_eq!(sub, decoded);
    }
}
