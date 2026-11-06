use crate::metadata::MetadataSummary;
use cassiopeia_geometry::geometry::NgsiLdGeometry;
use cassiopeia_ir::attribute_content::AttributeContent;
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
    value::{json_payload::JsonPayload, types::Value},
};
use iri_rs::IriBuf;
use urn_rs::Urn;

/// Builds a Property carrying the value and every qualifier its metadata declared.
#[must_use]
pub fn build_property(value: Value, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let mut builder = PropertyBuilder::new(value);
    if let Some(value_type) = meta.value_type {
        builder = builder.value_type(value_type);
    }
    if let Some(observed_at) = meta.observed_at {
        builder = builder.observed_at(observed_at);
    }
    if let Some(expires_at) = meta.expires_at {
        builder = builder.expires_at(expires_at);
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
    if let Some(expires_at) = meta.expires_at {
        builder = builder.expires_at(expires_at);
    }
    if let Some(dataset_id) = meta.dataset_id {
        builder = builder.dataset_id(dataset_id);
    }
    for (name, wrapper) in meta.custom_attributes {
        builder = builder.attribute(name, *wrapper);
    }

    builder.build().into_wrapper()
}

/// Builds a `GeoProperty` around one of the six geometry types ETSI GS CIM 009 v1.9.1 clause 4.7
/// admits.
///
/// A `GeoProperty` carries no nested attributes, so custom metadata is dropped.
///
/// The geometry is the one the extraction stage typed, already converted and normalised under the
/// declaration's own `geometry` policy, with any refusal already recorded there. It is taken exactly
/// as it is: parsing or normalising it again here would rewind the rings a `winding: keep` policy
/// left alone, and taking a typed geometry rather than a value means a `GeoProperty` can never be
/// built around something that is not one.
#[must_use]
pub fn build_geo_property(geometry: NgsiLdGeometry, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let mut geo = NgsiLdGeoProperty::new(geometry);
    geo.value_type = meta.value_type;
    geo.observed_at = meta.observed_at;
    geo.expires_at = meta.expires_at;
    geo.dataset_id = meta.dataset_id;

    geo.into_wrapper()
}

/// Builds a `LanguageProperty` around the `languageMap` extraction typed (ETSI GS CIM 009 v1.9.1
/// clause 4.5.18).
///
/// Extraction refuses, and records, a value that is no map of text keyed by language tag, so the map
/// is taken exactly as it is.
#[must_use]
pub fn build_language_property(language_map: LanguageMap, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let mut property = NgsiLdLanguageProperty::new(language_map);
    property.value_type = meta.value_type;
    property.observed_at = meta.observed_at;
    property.expires_at = meta.expires_at;
    property.dataset_id = meta.dataset_id;
    property.attributes = meta.custom_attributes;

    NgsiLdAttributeWrapper::single(NgsiLdAttribute::LanguageProperty(property))
}

/// Builds a `VocabProperty` around the vocabulary IRI extraction typed (ETSI GS CIM 009 v1.9.1
/// clause 4.5.20).
///
/// Extraction refuses, and records, a value that is no IRI, so the IRI is taken exactly as it is.
#[must_use]
pub fn build_vocab_property(has_vocab: IriBuf, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let mut property = NgsiLdVocabProperty::new(has_vocab);
    property.value_type = meta.value_type;
    property.observed_at = meta.observed_at;
    property.expires_at = meta.expires_at;
    property.dataset_id = meta.dataset_id;
    property.attributes = meta.custom_attributes;

    NgsiLdAttributeWrapper::single(NgsiLdAttribute::VocabProperty(property))
}

/// Builds a `ListProperty`, splitting an array value into its list and wrapping a scalar as a
/// single-element list.
#[must_use]
pub fn build_list_property(value: Value, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let has_value_list = match value {
        Value::Array(values) => values,
        Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Object(_) => vec![value],
    };

    let mut property = NgsiLdListProperty::new(has_value_list);
    property.value_type = meta.value_type;
    property.observed_at = meta.observed_at;
    property.expires_at = meta.expires_at;
    property.dataset_id = meta.dataset_id;
    property.attributes = meta.custom_attributes;

    NgsiLdAttributeWrapper::single(NgsiLdAttribute::ListProperty(property))
}

/// Builds a `JsonProperty` carrying its raw JSON verbatim.
#[must_use]
pub fn build_json_property(payload: JsonPayload, meta: MetadataSummary) -> NgsiLdAttributeWrapper {
    let mut property = NgsiLdJsonProperty::new(payload);
    property.value_type = meta.value_type;
    property.observed_at = meta.observed_at;
    property.expires_at = meta.expires_at;
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
    relationship.expires_at = meta.expires_at;
    relationship.attributes = meta.custom_attributes;

    NgsiLdAttributeWrapper::single(NgsiLdAttribute::ListRelationship(relationship))
}

/// Builds one sub-attribute from the NGSI-LD kind it was declared as, its content, its target type
/// (for a relationship), and its own metadata.
///
/// A sub-attribute is the serialization of a Property or any of its subclasses, and of a Relationship
/// (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 with 4.5.3), so every kind is dispatched to its own builder
/// and keeps the kind it declared rather than being flattened. Extraction types the content of a
/// `JsonProperty`, a `VocabProperty`, and a `LanguageProperty` as that kind, refusing and recording
/// a value the kind cannot hold, and those kinds are built only from that typed content. A builder
/// that rejects its content (a `Relationship` whose value is not a valid URN, an empty
/// `ListRelationship`, or content typed for another kind) yields `None`, dropping the sub-attribute
/// rather than emitting it malformed. A `GeoProperty` is built only around a geometry the extraction
/// stage already typed under its own `geometry` policy; nothing is parsed here. `object_type` is
/// consulted only by the two relationship kinds.
#[must_use]
pub fn build_sub_attribute(
    kind: NgsiLdAttributeKind,
    content: AttributeContent,
    object_type: Option<NameBuf>,
    meta: MetadataSummary,
) -> Option<NgsiLdAttributeWrapper> {
    let value = match content {
        AttributeContent::Value(value) => value,
        AttributeContent::Json(payload) => return matches!(kind, NgsiLdAttributeKind::JsonProperty).then(|| build_json_property(payload, meta)),
        AttributeContent::Vocab(iri) => return matches!(kind, NgsiLdAttributeKind::VocabProperty).then(|| build_vocab_property(iri, meta)),
        AttributeContent::Language(map) => return matches!(kind, NgsiLdAttributeKind::LanguageProperty).then(|| build_language_property(map, meta)),
    };
    match kind {
        NgsiLdAttributeKind::Property => Some(build_property(value, meta)),
        NgsiLdAttributeKind::GeoProperty => value.into_geometry().map(|geometry| build_geo_property(geometry, meta)),
        NgsiLdAttributeKind::ListProperty => Some(build_list_property(value, meta)),
        // Built from their typed content above, never from a Value.
        NgsiLdAttributeKind::JsonProperty | NgsiLdAttributeKind::VocabProperty | NgsiLdAttributeKind::LanguageProperty => None,
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
        attribute_builder::{
            build_geo_property,
            build_language_property,
            build_list_property,
            build_property,
            build_relationship,
            build_sub_attribute,
            build_vocab_property,
        },
        metadata::MetadataSummary,
    };
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use cassiopeia_ir::attribute_content::AttributeContent;
    use cassiopeia_ngsi_ld::{
        entity::{
            attribute::{NgsiLdAttribute, NgsiLdAttributeKind, NgsiLdAttributeWrapper, language_property::LanguageMap, property::NgsiLdProperty},
            name::NameBuf,
            value_type::ValueType,
        },
        value::{
            json_payload::JsonPayload,
            types::{Number, Value},
        },
    };
    use chrono::{DateTime, TimeZone, Utc};
    use indexmap::IndexMap;
    use iri_rs::IriBuf;
    use langtag::LangTagBuf;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use urn_rs::Urn;

    fn empty_meta() -> MetadataSummary {
        MetadataSummary {
            observed_at: None,
            expires_at: None,
            unit_code: None,
            dataset_id: None,
            custom_attributes: IndexMap::default(),
            value_type: None,
        }
    }

    fn expiring_meta(expires_at: DateTime<Utc>) -> MetadataSummary {
        MetadataSummary {
            expires_at: Some(expires_at),
            ..empty_meta()
        }
    }

    fn name(value: &str) -> NameBuf {
        NameBuf::new(value).unwrap()
    }

    #[test]
    fn a_property_and_a_relationship_carry_their_expires_at_qualifier() {
        let expires_at = Utc.with_ymd_and_hms(2026, 10, 25, 13, 0, 0).unwrap();

        let NgsiLdAttributeWrapper::Single(property) = build_property(Value::try_from(json!("ok")).unwrap(), expiring_meta(expires_at)) else {
            panic!("expected a single property");
        };
        let NgsiLdAttribute::Property(property) = property.as_ref() else {
            panic!("expected a property");
        };
        assert_eq!(property.expires_at, Some(expires_at));

        let object = "urn:ngsi-ld:Vehicle:car123".parse::<Urn>().unwrap();
        let NgsiLdAttributeWrapper::Single(relationship) = build_relationship(object, None, expiring_meta(expires_at)) else {
            panic!("expected a single relationship");
        };
        let NgsiLdAttribute::Relationship(relationship) = relationship.as_ref() else {
            panic!("expected a relationship");
        };
        assert_eq!(relationship.expires_at, Some(expires_at));
    }

    #[test]
    fn a_geo_property_carries_its_expires_at_qualifier() {
        let expires_at = Utc.with_ymd_and_hms(2026, 10, 25, 13, 0, 0).unwrap();

        let NgsiLdAttributeWrapper::Single(attr) = build_geo_property(clockwise_square(), expiring_meta(expires_at)) else {
            panic!("expected a single geo property");
        };
        let NgsiLdAttribute::GeoProperty(geo) = attr.as_ref() else {
            panic!("expected a geo property");
        };
        assert_eq!(geo.expires_at, Some(expires_at));
    }

    #[test]
    fn a_language_property_carries_the_language_map_it_was_built_around() {
        let mut map = LanguageMap::default();
        map.insert(LangTagBuf::new("en".to_owned()).unwrap(), "hello".to_owned());

        let NgsiLdAttributeWrapper::Single(attr) = build_language_property(map.clone(), empty_meta()) else {
            panic!("expected a single language property");
        };
        let NgsiLdAttribute::LanguageProperty(property) = attr.as_ref() else {
            panic!("expected a language property");
        };
        assert_eq!(property.language_map, map);
    }

    /// A polygon whose exterior ring is wound clockwise, against RFC 7946 clause 3.1.6's right-hand
    /// rule, as a `winding: keep` policy leaves it.
    fn clockwise_square() -> NgsiLdGeometry {
        NgsiLdGeometry::Polygon {
            coordinates: vec![vec![
                [0.0, 0.0].into(),
                [0.0, 2.0].into(),
                [2.0, 2.0].into(),
                [2.0, 0.0].into(),
                [0.0, 0.0].into(),
            ]],
        }
    }

    /// The geometry a built `GeoProperty` carries.
    fn geometry_of(wrapper: NgsiLdAttributeWrapper) -> NgsiLdGeometry {
        let NgsiLdAttributeWrapper::Single(attr) = wrapper else {
            panic!("expected a single geo property");
        };
        let NgsiLdAttribute::GeoProperty(geo) = *attr else {
            panic!("expected a geo property");
        };
        geo.value
    }

    #[test]
    fn a_geo_property_takes_its_typed_geometry_exactly_as_it_is() {
        assert_eq!(geometry_of(build_geo_property(clockwise_square(), empty_meta())), clockwise_square());
    }

    #[test]
    fn a_geo_property_sub_attribute_keeps_the_geometry_extraction_typed_it_as() {
        let wrapper = build_sub_attribute(
            NgsiLdAttributeKind::GeoProperty,
            AttributeContent::Value(Value::Geospatial(Box::new(clockwise_square()))),
            None,
            empty_meta(),
        )
        .expect("geo property built");

        assert_eq!(geometry_of(wrapper), clockwise_square());
    }

    #[test]
    fn a_geo_property_sub_attribute_whose_value_is_no_typed_geometry_is_dropped() {
        // Extraction types every geometry a GeoProperty carries, so a value that reaches the builder
        // untyped (GeoJSON text or a GeoJSON-shaped object included) is never parsed into one here.
        for value in [
            Value::String("not a geometry".into()),
            Value::String(r#"{"type":"Point","coordinates":[1.5,2.5]}"#.into()),
            Value::try_from(json!({"type": "Point", "coordinates": [1.5, 2.5]})).unwrap(),
        ] {
            assert!(
                build_sub_attribute(NgsiLdAttributeKind::GeoProperty, AttributeContent::Value(value.clone()), None, empty_meta()).is_none(),
                "{value:?}"
            );
        }
    }

    #[test]
    fn a_vocab_property_carries_the_iri_it_was_built_around() {
        let iri = IriBuf::new("https://example.org/level/high".to_owned()).unwrap();

        let NgsiLdAttributeWrapper::Single(attr) = build_vocab_property(iri.clone(), empty_meta()) else {
            panic!("expected a single vocab property");
        };
        let NgsiLdAttribute::VocabProperty(property) = attr.as_ref() else {
            panic!("expected a vocab property");
        };
        assert_eq!(property.has_vocab, iri);
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
            AttributeContent::Value(Value::String("urn:ngsi-ld:Character:JackSparrow".into())),
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
        assert!(
            build_sub_attribute(
                NgsiLdAttributeKind::Relationship,
                AttributeContent::Value(Value::String("not a urn".into())),
                None,
                empty_meta()
            )
            .is_none()
        );
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
            expires_at: None,
            unit_code: None,
            dataset_id: None,
            custom_attributes: custom,
            value_type: None,
        };

        let wrapper = build_sub_attribute(
            NgsiLdAttributeKind::ListRelationship,
            AttributeContent::Value(Value::Array(vec![
                Value::String("urn:ngsi-ld:Person:1".into()),
                Value::String("urn:ngsi-ld:Person:2".into()),
            ])),
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
        assert!(
            build_sub_attribute(
                NgsiLdAttributeKind::ListRelationship,
                AttributeContent::Value(Value::Array(vec![])),
                Some(name("Person")),
                empty_meta()
            )
            .is_none()
        );
    }

    #[test]
    fn a_json_property_sub_attribute_carries_its_raw_json_with_a_nested_null() {
        let payload = JsonPayload::try_from(json!({"maybe": null})).unwrap();
        let wrapper =
            build_sub_attribute(NgsiLdAttributeKind::JsonProperty, AttributeContent::Json(payload.clone()), None, empty_meta()).expect("json property built");

        let NgsiLdAttributeWrapper::Single(attr) = wrapper else {
            panic!("expected a single json property");
        };
        let NgsiLdAttribute::JsonProperty(property) = attr.as_ref() else {
            panic!("expected a json property");
        };
        assert_eq!(property.has_json, payload);
    }

    #[test]
    fn a_json_property_sub_attribute_whose_content_is_a_value_is_dropped() {
        assert!(
            build_sub_attribute(
                NgsiLdAttributeKind::JsonProperty,
                AttributeContent::Value(Value::Boolean(true)),
                None,
                empty_meta()
            )
            .is_none()
        );
    }

    #[test]
    fn a_property_sub_attribute_whose_content_is_raw_json_is_dropped() {
        let payload = JsonPayload::try_from(json!({"a": 1})).unwrap();

        assert!(build_sub_attribute(NgsiLdAttributeKind::Property, AttributeContent::Json(payload), None, empty_meta()).is_none());
    }

    #[test]
    fn vocab_and_language_sub_attributes_are_built_from_their_typed_content() {
        let iri = IriBuf::new("https://example.org/level/high".to_owned()).unwrap();
        let mut map = LanguageMap::default();
        map.insert(LangTagBuf::new("en".to_owned()).unwrap(), "hello".to_owned());

        let vocab = build_sub_attribute(NgsiLdAttributeKind::VocabProperty, AttributeContent::Vocab(iri), None, empty_meta()).expect("vocab built");
        let language = build_sub_attribute(NgsiLdAttributeKind::LanguageProperty, AttributeContent::Language(map), None, empty_meta()).expect("language built");

        assert!(matches!(vocab, NgsiLdAttributeWrapper::Single(attr) if matches!(*attr, NgsiLdAttribute::VocabProperty(_))));
        assert!(matches!(language, NgsiLdAttributeWrapper::Single(attr) if matches!(*attr, NgsiLdAttribute::LanguageProperty(_))));
    }

    #[test]
    fn a_vocab_or_language_sub_attribute_whose_content_is_a_value_is_dropped() {
        for kind in [NgsiLdAttributeKind::VocabProperty, NgsiLdAttributeKind::LanguageProperty] {
            assert!(build_sub_attribute(kind, AttributeContent::Value(Value::String("https://example.org/x".into())), None, empty_meta()).is_none());
        }
    }

    #[test]
    fn typed_content_under_another_kind_is_dropped() {
        let iri = IriBuf::new("https://example.org/level/high".to_owned()).unwrap();

        assert!(build_sub_attribute(NgsiLdAttributeKind::Property, AttributeContent::Vocab(iri.clone()), None, empty_meta()).is_none());
        assert!(build_sub_attribute(NgsiLdAttributeKind::LanguageProperty, AttributeContent::Vocab(iri), None, empty_meta()).is_none());
    }

    #[test]
    fn every_property_kind_writes_the_value_type_its_metadata_carries() {
        let declared = ValueType::new("Thing").unwrap();
        let meta = || empty_meta().with_value_type(Some(declared.clone()));

        let NgsiLdAttributeWrapper::Single(property) = build_property(Value::String("a".into()), meta()) else {
            panic!("expected a single property");
        };
        let NgsiLdAttribute::Property(property) = *property else {
            panic!("expected a property");
        };
        assert_eq!(property.value_type.as_ref(), Some(&declared));

        let NgsiLdAttributeWrapper::Single(list) = build_list_property(Value::String("a".into()), meta()) else {
            panic!("expected a single list property");
        };
        let NgsiLdAttribute::ListProperty(list) = *list else {
            panic!("expected a list property");
        };
        assert_eq!(list.value_type.as_ref(), Some(&declared));

        let NgsiLdAttributeWrapper::Single(vocab) = build_vocab_property(IriBuf::new("https://example.org/x".to_owned()).unwrap(), meta()) else {
            panic!("expected a single vocab property");
        };
        let NgsiLdAttribute::VocabProperty(vocab) = *vocab else {
            panic!("expected a vocab property");
        };
        assert_eq!(vocab.value_type.as_ref(), Some(&declared));
    }
}
