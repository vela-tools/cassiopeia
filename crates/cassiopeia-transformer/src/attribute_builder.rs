use crate::metadata::MetadataSummary;
use cassiopeia_geometry::{policy::GeometryPolicy, target::GeometryTarget};
use cassiopeia_ngsi_ld::{
    entity::{
        attribute::{
            NgsiLdAttribute,
            NgsiLdAttributeKind,
            NgsiLdAttributeWrapper,
            geo_property::NgsiLdGeoProperty,
            json_property::NgsiLdJsonProperty,
            language_property::{LanguageMap, NgsiLdLanguageProperty},
            list_property::NgsiLdListProperty,
            list_relationship::NgsiLdListRelationship,
            vocab_property::NgsiLdVocabProperty,
        },
        builder::attribute::{IntoAttributeWrapper, PropertyBuilder, RelationshipBuilder},
        name::NameBuf,
    },
    value::types::Value,
};
use iri_rs::IriBuf;
use langtag::LangTagBuf;
use urn_rs::Urn;

/// Builds a Property carrying the value and every qualifier its metadata declared.
#[must_use]
pub fn build_property(value: Value, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let mut builder = PropertyBuilder::new(value);
    if let Some(observed_at) = meta.observed_at {
        builder = builder.observed_at(observed_at);
    }
    if let Some(unit_code) = meta.unit_code {
        builder = builder.unit_code(unit_code);
    }
    if let Some(dataset_id) = meta.dataset_id {
        builder = builder.dataset_id(dataset_id);
    }
    for (name, wrapper) in meta.custom_attributes {
        builder = builder.attribute(name, *wrapper);
    }

    builder.build().into_wrapper()
}

/// Builds a Relationship pointing at one object URN.
#[must_use]
pub fn build_relationship(object: Urn, object_type: Option<NameBuf>, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let mut builder = RelationshipBuilder::new(object);
    if let Some(object_type) = object_type {
        builder = builder.object_type(object_type);
    }
    if let Some(observed_at) = meta.observed_at {
        builder = builder.observed_at(observed_at);
    }
    if let Some(dataset_id) = meta.dataset_id {
        builder = builder.dataset_id(dataset_id);
    }
    for (name, wrapper) in meta.custom_attributes {
        builder = builder.attribute(name, *wrapper);
    }

    builder.build().into_wrapper()
}

/// Builds a `GeoProperty` from a value carrying one of the six geometry types clause 4.7 admits.
///
/// A `GeoProperty` carries no nested attributes, so custom metadata is dropped.
///
/// The geometry is resolved preserving whatever type the source carried, so a mapping that declares
/// `type: "GeoProperty"` and no `transformation` still works: the value the extraction stage produced
/// may be an already-typed geometry, or the `GeoJSON` text a source wrote and no transformation
/// parsed. A value carrying no admissible geometry yields no attribute, so a `GeoProperty` is never
/// emitted around something that is not a geometry.
#[must_use]
pub fn build_geo_property(value: &Value, meta: MetadataSummary) -> Option<NgsiLdAttributeWrapper> {
    let geometry = value.to_geometry(GeometryTarget::Preserve, &GeometryPolicy::default()).ok()??;

    let mut geo = NgsiLdGeoProperty::new(geometry);
    geo.observed_at = meta.observed_at;
    geo.dataset_id = meta.dataset_id;

    Some(geo.into_wrapper())
}

/// Builds a `LanguageProperty` from a value that is an object of language tag to text.
///
/// A value that is not an object yields no attribute: a `languageMap` is defined only over a set of
/// language-tagged strings (ETSI GS CIM 009 v1.9.1 clause 4.5.18).
#[must_use]
pub fn build_language_property(value: Value, meta: MetadataSummary) -> Option<NgsiLdAttributeWrapper> {
    let mut language_map = LanguageMap::default();
    match value {
        Value::Object(entries) => {
            for (language, text) in entries.as_ref() {
                // Only well-formed BCP-47 tags with textual values enter the map.
                if let (Ok(tag), Some(text)) = (LangTagBuf::new(language.to_string()), text.as_str()) {
                    language_map.insert(tag, text.to_string());
                }
            }
        }
        Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) => {
            return None;
        }
    }

    let mut property = NgsiLdLanguageProperty::new(language_map);
    property.observed_at = meta.observed_at;
    property.dataset_id = meta.dataset_id;
    property.attributes = meta.custom_attributes;

    Some(NgsiLdAttributeWrapper::single(NgsiLdAttribute::LanguageProperty(property)))
}

/// Builds a `VocabProperty` from the value's string form.
///
/// A value that is not a valid IRI yields no attribute: `vocab` is a mapping to a vocabulary IRI
/// (ETSI GS CIM 009 v1.9.1 clause 4.5.20), so a non-IRI value is dropped rather than emitted.
#[must_use]
pub fn build_vocab_property(value: &Value, meta: MetadataSummary) -> Option<NgsiLdAttributeWrapper> {
    let has_vocab = IriBuf::new(value.to_string()).ok()?;
    let mut property = NgsiLdVocabProperty::new(has_vocab);
    property.observed_at = meta.observed_at;
    property.dataset_id = meta.dataset_id;
    property.attributes = meta.custom_attributes;

    Some(NgsiLdAttributeWrapper::single(NgsiLdAttribute::VocabProperty(property)))
}

/// Builds a `ListProperty`, splitting an array value into its list and wrapping a scalar as a
/// single-element list.
#[must_use]
pub fn build_list_property(value: Value, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let has_value_list = match value {
        Value::Array(values) => values,
        Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Object(_) => vec![value],
    };

    let mut property = NgsiLdListProperty::new(has_value_list);
    property.observed_at = meta.observed_at;
    property.dataset_id = meta.dataset_id;
    property.attributes = meta.custom_attributes;

    NgsiLdAttributeWrapper::single(NgsiLdAttribute::ListProperty(property))
}

/// Builds a `JsonProperty` carrying the value verbatim.
#[must_use]
pub fn build_json_property(value: Value, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let mut property = NgsiLdJsonProperty::new(value);
    property.observed_at = meta.observed_at;
    property.dataset_id = meta.dataset_id;
    property.attributes = meta.custom_attributes;

    NgsiLdAttributeWrapper::single(NgsiLdAttribute::JsonProperty(property))
}

/// Builds a `ListRelationship` pointing at every object URN, carrying its shared qualifiers and any
/// sub-attributes qualifying the list as a whole (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2).
#[must_use]
pub fn build_list_relationship(objects: Vec<Urn>, object_type: Option<NameBuf>, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let mut relationship = NgsiLdListRelationship::new(objects);
    relationship.object_type = object_type;
    relationship.observed_at = meta.observed_at;
    relationship.attributes = meta.custom_attributes;

    NgsiLdAttributeWrapper::single(NgsiLdAttribute::ListRelationship(relationship))
}

/// Builds one sub-attribute from the NGSI-LD kind it was declared as, its value, its target type (for
/// a relationship), and its own metadata.
///
/// A sub-attribute is the serialization of a Property or any of its subclasses, and of a Relationship
/// (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3), so every kind is dispatched to its own builder
/// and keeps the kind it declared rather than being flattened. A builder that rejects its value (a
/// non-IRI `VocabProperty`, a non-object `LanguageProperty`, a `Relationship` whose value is not a
/// valid URN, an empty `ListRelationship`) yields `None`, dropping the sub-attribute rather than
/// emitting it malformed. `object_type` is consulted only by the two relationship kinds.
#[must_use]
pub fn build_sub_attribute(kind: NgsiLdAttributeKind, value: Value, object_type: Option<NameBuf>, meta: MetadataSummary) -> Option<NgsiLdAttributeWrapper> {
    match kind {
        NgsiLdAttributeKind::Property => Some(build_property(value, meta)),
        NgsiLdAttributeKind::GeoProperty => build_geo_property(&value, meta),
        NgsiLdAttributeKind::ListProperty => Some(build_list_property(value, meta)),
        NgsiLdAttributeKind::JsonProperty => Some(build_json_property(value, meta)),
        NgsiLdAttributeKind::VocabProperty => build_vocab_property(&value, meta),
        NgsiLdAttributeKind::LanguageProperty => build_language_property(value, meta),
        NgsiLdAttributeKind::Relationship => {
            // A relationship sub-attribute's value is its object URN; a value that is not a valid URN
            // is dropped rather than emitted malformed.
            let object = value.as_str()?.parse::<Urn>().ok()?;
            Some(build_relationship(object, object_type, meta))
        }
        NgsiLdAttributeKind::ListRelationship => {
            let Value::Array(items) = value else {
                return None;
            };
            let objects: Vec<Urn> = items.iter().filter_map(|item| item.as_str()?.parse::<Urn>().ok()).collect();
            if objects.is_empty() {
                return None;
            }
            Some(build_list_relationship(objects, object_type, meta))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_builder::{build_geo_property, build_language_property, build_list_property, build_sub_attribute, build_vocab_property},
        metadata::MetadataSummary,
    };
    use cassiopeia_ngsi_ld::{
        entity::{
            attribute::{NgsiLdAttribute, NgsiLdAttributeKind, NgsiLdAttributeWrapper, property::NgsiLdProperty},
            name::NameBuf,
        },
        value::types::{Number, Value},
    };
    use indexmap::IndexMap;

    fn empty_meta() -> MetadataSummary {
        MetadataSummary {
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            custom_attributes: IndexMap::default(),
        }
    }

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    #[test]
    fn a_language_property_rejects_a_value_that_is_not_an_object() {
        assert!(build_language_property(Value::String("hello".into()), empty_meta()).is_none());
    }

    #[test]
    fn a_geo_property_rejects_a_value_that_carries_no_geometry() {
        // A GeoProperty holds one of the six geometry types clause 4.7 admits, which has no null or
        // empty form, so a value that is not a geometry yields no attribute at all rather than an
        // attribute whose value is null.
        assert!(build_geo_property(&Value::String("not a geometry".into()), empty_meta()).is_none());
        assert!(build_geo_property(&Value::Null, empty_meta()).is_none());
    }

    #[test]
    fn a_geo_property_reads_the_geojson_text_a_source_wrote() {
        let value = Value::String(r#"{"type":"Point","coordinates":[1.5,2.5]}"#.into());

        assert!(build_geo_property(&value, empty_meta()).is_some());
    }

    #[test]
    fn a_geo_property_sub_attribute_carrying_no_geometry_is_dropped() {
        let dropped = build_sub_attribute(NgsiLdAttributeKind::GeoProperty, Value::String("not a geometry".into()), None, empty_meta());

        assert!(dropped.is_none());
    }

    #[test]
    fn a_vocab_property_rejects_a_value_that_is_not_a_valid_iri() {
        assert!(build_vocab_property(&Value::String(" not an iri ".into()), empty_meta()).is_none());
    }

    #[test]
    fn a_list_property_wraps_a_scalar_as_a_single_element_list() {
        let NgsiLdAttributeWrapper::Single(attr) = build_list_property(Value::Number(Number::Integer(7)), empty_meta()) else {
            panic!("expected a single list property");
        };
        let NgsiLdAttribute::ListProperty(property) = attr.as_ref() else {
            panic!("expected a list property");
        };
        assert_eq!(property.has_value_list.len(), 1);
    }

    #[test]
    fn a_relationship_sub_attribute_carries_its_object_and_object_type() {
        let wrapper = build_sub_attribute(
            NgsiLdAttributeKind::Relationship,
            Value::String("urn:ngsi-ld:Character:JackSparrow".into()),
            Some(name("Character")),
            empty_meta(),
        )
        .expect("relationship built");

        let NgsiLdAttributeWrapper::Single(attr) = wrapper else {
            panic!("expected a single relationship");
        };
        let NgsiLdAttribute::Relationship(relationship) = attr.as_ref() else {
            panic!("expected a relationship");
        };
        assert_eq!(relationship.object.to_string(), "urn:ngsi-ld:Character:JackSparrow");
        assert_eq!(relationship.object_type, Some(name("Character")));
    }

    #[test]
    fn a_relationship_sub_attribute_rejects_a_non_urn_value() {
        assert!(build_sub_attribute(NgsiLdAttributeKind::Relationship, Value::String("not a urn".into()), None, empty_meta()).is_none());
    }

    #[test]
    fn a_list_relationship_sub_attribute_carries_objects_object_type_and_attributes() {
        let mut custom = IndexMap::default();
        custom.insert(
            name("dataProvider"),
            Box::new(NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(Value::String(
                "acme".into(),
            ))))),
        );
        let meta = MetadataSummary {
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            custom_attributes: custom,
        };

        let wrapper = build_sub_attribute(
            NgsiLdAttributeKind::ListRelationship,
            Value::Array(vec![Value::String("urn:ngsi-ld:Person:1".into()), Value::String("urn:ngsi-ld:Person:2".into())]),
            Some(name("Person")),
            meta,
        )
        .expect("list relationship built");

        let NgsiLdAttributeWrapper::Single(attr) = wrapper else {
            panic!("expected a single list relationship");
        };
        let NgsiLdAttribute::ListRelationship(list) = attr.as_ref() else {
            panic!("expected a list relationship");
        };
        assert_eq!(list.object_list.len(), 2);
        assert_eq!(list.object_type, Some(name("Person")));
        assert!(list.attributes.contains_key(&name("dataProvider")));
    }

    #[test]
    fn an_empty_list_relationship_sub_attribute_is_dropped() {
        assert!(build_sub_attribute(NgsiLdAttributeKind::ListRelationship, Value::Array(vec![]), Some(name("Person")), empty_meta()).is_none());
    }
}
