use crate::{
    entity::{
        NgsiLdEntity,
        attribute::{
            Attributes,
            NgsiLdAttribute,
            NgsiLdAttributeWrapper,
            geo_property::NgsiLdGeoProperty,
            json_property::NgsiLdJsonProperty,
            list_property::NgsiLdListProperty,
            list_relationship::NgsiLdListRelationship,
            property::NgsiLdProperty,
            relationship::NgsiLdRelationship,
            vocab_property::NgsiLdVocabProperty,
        },
        context::{NgsiLdContext, NgsiLdContextEntry},
        name::NameBuf,
        representation::{JsonLayout, NgsiLdSerializable},
    },
    value::types::Value,
};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use cassiopeia_geometry::geometry::NgsiLdGeometry;
use chrono::{TimeZone, Utc};
use indexmap::IndexMap;
use iri_rs::IriBuf;
use serde_json::{Value as JsonValue, json};
use url::Url;
use urn_rs::Urn;

/// A single-remote-URL `@context` built from a string literal.
fn url_ctx(s: &str) -> NgsiLdContext {
    NgsiLdContext::remote(Url::parse(s).unwrap())
}

fn create_test_entity(id: &str, entity_type: &str, key: &str, attr: NgsiLdAttribute) -> NgsiLdEntity {
    let mut attributes = IndexMap::default();
    attributes.insert(NameBuf::new(key).unwrap(), NgsiLdAttributeWrapper::single(attr));

    NgsiLdEntity {
        id: format!("urn:ngsi-ld:{entity_type}:{id}").parse::<Urn>().unwrap(),
        entity_type: NameBuf::new(entity_type).unwrap(),
        context: Some(url_ctx("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld")),
        scope: None,
        attributes,
    }
}

fn serialize_entity(entity: &NgsiLdEntity, mode: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> JsonValue {
    entity.to_json(mode, skip_null).unwrap()
}

fn serialize_entities(entities: &[NgsiLdEntity], mode: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> JsonValue {
    entities.to_vec().to_json(mode, skip_null).unwrap()
}

mod property_tests {
    use super::*;
    use cefact_units::UnitCode;

    #[test]
    fn test_simple_property_all_modes() {
        let prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::String("John Doe".to_string())),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });
        let entity = create_test_entity("001", "Person", "name", prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["name"]["type"], "Property");
        assert_eq!(normalized["name"]["value"], "John Doe");
        assert!(!normalized["name"].as_object().unwrap().contains_key("observedAt"));
        assert!(!normalized["name"].as_object().unwrap().contains_key("unitCode"));

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["name"], "John Doe");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["name"], "John Doe");
    }

    #[test]
    fn test_property_with_metadata() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 0).unwrap();
        let prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(json!(25)),
            observed_at: Some(dt),
            unit_code: Some(UnitCode::Cel),
            dataset_id: Some("urn:dataset:temp".parse().unwrap()),
            instance_id: Some("urn:instance:123".parse().unwrap()),
            attributes: IndexMap::default(),
        });
        let entity = create_test_entity("001", "Sensor", "temperature", prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["temperature"]["type"], "Property");
        assert_eq!(normalized["temperature"]["value"], 25);
        assert_eq!(normalized["temperature"]["unitCode"], "CEL");
        assert_eq!(normalized["temperature"]["datasetId"], "urn:dataset:temp");
        assert_eq!(normalized["temperature"]["instanceId"], "urn:instance:123");
        assert_eq!(normalized["temperature"]["observedAt"], "2023-12-25T10:30:00Z");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["temperature"]["value"], 25);
        assert_eq!(concise["temperature"]["unitCode"], "CEL");
        assert_eq!(concise["temperature"]["observedAt"], "2023-12-25T10:30:00Z");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["temperature"], 25);
    }

    #[test]
    fn test_property_with_nested_properties() {
        let accuracy_prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Number(95.into())),
            observed_at: None,
            unit_code: Some(UnitCode::P1),
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });

        let mut nested_props = IndexMap::default();
        nested_props.insert(NameBuf::new("accuracy").unwrap(), Box::new(NgsiLdAttributeWrapper::single(accuracy_prop)));

        let main_prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Number(100.into())),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: nested_props,
        });
        let entity = create_test_entity("001", "Sensor", "reading", main_prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["reading"]["type"], "Property");
        assert_eq!(normalized["reading"]["value"], 100);
        assert_eq!(normalized["reading"]["accuracy"]["type"], "Property");

        let accuracy_value = &normalized["reading"]["accuracy"]["value"];
        assert_eq!(accuracy_value, &json!(95));
        assert_eq!(normalized["reading"]["accuracy"]["unitCode"], "P1");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["reading"]["value"], 100);
        assert_eq!(concise["reading"]["accuracy"]["value"], json!(95));
        assert_eq!(concise["reading"]["accuracy"]["unitCode"], "P1");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["reading"], 100);
    }

    #[test]
    fn test_property_null_values() {
        let prop_null = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Null),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });
        let entity_null = create_test_entity("001", "Test", "nullValue", prop_null);

        let include_null = serialize_entity(&entity_null, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Include);
        assert_eq!(include_null["nullValue"]["type"], "Property");
        assert!(include_null["nullValue"]["value"].is_null());

        let skip_null = serialize_entity(&entity_null, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert!(!skip_null.as_object().unwrap().contains_key("nullValue"));

        let simplified_skip = serialize_entity(&entity_null, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert!(!simplified_skip.as_object().unwrap().contains_key("nullValue"));
    }
}

mod relationship_tests {
    use super::*;

    #[test]
    fn test_simple_relationship_all_modes() {
        let rel = NgsiLdAttribute::Relationship(NgsiLdRelationship {
            object: "urn:ngsi-ld:Vehicle:car123".parse::<Urn>().unwrap(),
            object_type: None,
            observed_at: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });
        let entity = create_test_entity("001", "Person", "drives", rel);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["drives"]["type"], "Relationship");
        assert_eq!(normalized["drives"]["object"], "urn:ngsi-ld:Vehicle:car123");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["drives"], "urn:ngsi-ld:Vehicle:car123");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["drives"], "urn:ngsi-ld:Vehicle:car123");
    }

    #[test]
    fn test_relationship_with_metadata() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 0).unwrap();
        let mut rel_props = IndexMap::default();

        let count_prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Number(5.into())),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });
        rel_props.insert(NameBuf::new("count").unwrap(), Box::new(NgsiLdAttributeWrapper::single(count_prop)));

        let rel = NgsiLdAttribute::Relationship(NgsiLdRelationship {
            object: "urn:ngsi-ld:Organization:Beatles".parse::<Urn>().unwrap(),
            object_type: None,
            observed_at: Some(dt),
            dataset_id: Some("urn:ngsi-ld:Relationship:Beatles".parse().unwrap()),
            instance_id: None,
            attributes: rel_props,
        });
        let entity = create_test_entity("001", "Person", "memberOf", rel);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["memberOf"]["type"], "Relationship");
        assert_eq!(normalized["memberOf"]["object"], "urn:ngsi-ld:Organization:Beatles");
        assert_eq!(normalized["memberOf"]["observedAt"], "2023-12-25T10:30:00Z");
        assert_eq!(normalized["memberOf"]["datasetId"], "urn:ngsi-ld:Relationship:Beatles");
        assert_eq!(normalized["memberOf"]["count"]["type"], "Property");
        assert_eq!(normalized["memberOf"]["count"]["value"], 5);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["memberOf"]["object"], "urn:ngsi-ld:Organization:Beatles");
        assert_eq!(concise["memberOf"]["observedAt"], "2023-12-25T10:30:00Z");
        assert_eq!(concise["memberOf"]["count"], 5);

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["memberOf"], "urn:ngsi-ld:Organization:Beatles");
    }
}

mod geoproperty_tests {
    use super::*;

    #[test]
    fn test_geoproperty_all_modes() {
        let geo_value = NgsiLdGeometry::Point {
            coordinates: [-73.975, 40.775_556].into(),
        };

        let geo = NgsiLdAttribute::GeoProperty(NgsiLdGeoProperty {
            value: geo_value,
            observed_at: None,
            dataset_id: None,
            instance_id: None,
        });
        let entity = create_test_entity("001", "Place", "location", geo);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["location"]["type"], "GeoProperty");
        assert_eq!(normalized["location"]["value"]["type"], "Point");
        assert_eq!(normalized["location"]["value"]["coordinates"][0], -73.975);
        assert_eq!(normalized["location"]["value"]["coordinates"][1], 40.775_556);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["location"]["type"], "Point");
        assert_eq!(concise["location"]["coordinates"][0], -73.975);

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["location"]["type"], "Point");
        assert_eq!(simplified["location"]["coordinates"][0], -73.975);
    }

    #[test]
    fn test_geoproperty_with_metadata() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 0).unwrap();
        let geo_value = NgsiLdGeometry::Polygon {
            coordinates: vec![vec![
                [-73.975, 40.775_556].into(),
                [-73.975, 40.775_557].into(),
                [-73.974, 40.775_557].into(),
                [-73.974, 40.775_556].into(),
                [-73.975, 40.775_556].into(),
            ]],
        };

        let geo = NgsiLdAttribute::GeoProperty(NgsiLdGeoProperty {
            value: geo_value,
            observed_at: Some(dt),
            dataset_id: Some("urn:dataset:geo".parse().unwrap()),
            instance_id: None,
        });
        let entity = create_test_entity("001", "Place", "area", geo);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["area"]["type"], "GeoProperty");
        assert_eq!(normalized["area"]["value"]["type"], "Polygon");
        assert_eq!(normalized["area"]["observedAt"], "2023-12-25T10:30:00Z");
        assert_eq!(normalized["area"]["datasetId"], "urn:dataset:geo");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["area"]["value"]["type"], "Polygon");
        assert_eq!(concise["area"]["observedAt"], "2023-12-25T10:30:00Z");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["area"]["type"], "Polygon");
    }
}

mod list_relationship_tests {
    use super::*;

    #[test]
    fn test_simple_list_relationship_all_modes() {
        let list_rel = NgsiLdAttribute::ListRelationship(NgsiLdListRelationship {
            object_list: vec![
                "urn:ngsi-ld:Person:John".parse::<Urn>().unwrap(),
                "urn:ngsi-ld:Person:Paul".parse::<Urn>().unwrap(),
                "urn:ngsi-ld:Person:George".parse::<Urn>().unwrap(),
            ],
            object_type: None,
            observed_at: None,
            dataset_id: None,
            attributes: IndexMap::default(),
        });
        let entity = create_test_entity("001", "Group", "members", list_rel);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["members"]["type"], "ListRelationship");
        assert_eq!(normalized["members"]["objectList"][0], "urn:ngsi-ld:Person:John");
        assert_eq!(normalized["members"]["objectList"][1], "urn:ngsi-ld:Person:Paul");
        assert_eq!(normalized["members"]["objectList"][2], "urn:ngsi-ld:Person:George");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["members"]["objectList"][0], "urn:ngsi-ld:Person:John");
        assert_eq!(concise["members"]["objectList"][1], "urn:ngsi-ld:Person:Paul");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert!(simplified["members"].is_array());
        assert_eq!(simplified["members"][0], "urn:ngsi-ld:Person:John");
        assert_eq!(simplified["members"][1], "urn:ngsi-ld:Person:Paul");
        assert_eq!(simplified["members"][2], "urn:ngsi-ld:Person:George");
    }

    #[test]
    fn test_list_relationship_with_single_item() {
        let list_rel = NgsiLdAttribute::ListRelationship(NgsiLdListRelationship {
            object_list: vec!["urn:ngsi-ld:Organization:ComuneDiMilano".parse::<Urn>().unwrap()],
            object_type: None,
            observed_at: None,
            dataset_id: None,
            attributes: IndexMap::default(),
        });
        let entity = create_test_entity("001", "Asset", "owner", list_rel);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["owner"]["type"], "ListRelationship");
        assert_eq!(normalized["owner"]["objectList"][0], "urn:ngsi-ld:Organization:ComuneDiMilano");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert!(simplified["owner"].is_array());
        assert_eq!(simplified["owner"][0], "urn:ngsi-ld:Organization:ComuneDiMilano");
    }

    #[test]
    fn a_list_relationship_serializes_its_sub_attributes() {
        let mut attributes = IndexMap::default();
        attributes.insert(
            NameBuf::new("castSize").unwrap(),
            Box::new(NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(2)))),
        );
        let list_rel = NgsiLdAttribute::ListRelationship(NgsiLdListRelationship {
            object_list: vec!["urn:ngsi-ld:Person:1".parse::<Urn>().unwrap()],
            object_type: Some(NameBuf::new("Person").unwrap()),
            observed_at: None,
            dataset_id: None,
            attributes,
        });
        let entity = create_test_entity("001", "Movie", "hasCast", list_rel);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["hasCast"]["type"], "ListRelationship");
        assert_eq!(normalized["hasCast"]["castSize"]["type"], "Property");
        assert_eq!(normalized["hasCast"]["castSize"]["value"], 2);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["hasCast"]["castSize"], 2);

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert!(simplified["hasCast"].is_array());
        assert_eq!(simplified["hasCast"][0], "urn:ngsi-ld:Person:1");
    }

    #[test]
    fn a_list_relationship_without_sub_attributes_carries_no_extra_keys() {
        let list_rel = NgsiLdAttribute::ListRelationship(NgsiLdListRelationship::new(vec!["urn:ngsi-ld:Person:1".parse::<Urn>().unwrap()]));
        let entity = create_test_entity("001", "Movie", "hasCast", list_rel);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        let object = normalized["hasCast"].as_object().unwrap();
        let keys: Vec<&str> = object.keys().map(String::as_str).collect();
        assert_eq!(keys, ["type", "objectList"]);
    }

    #[test]
    fn a_relationship_holding_a_nested_relationship_serializes_the_nested_object() {
        let mut attributes = IndexMap::default();
        attributes.insert(
            NameBuf::new("playsCharacter").unwrap(),
            Box::new(NgsiLdAttributeWrapper::single(NgsiLdAttribute::Relationship(NgsiLdRelationship {
                object: "urn:ngsi-ld:Character:JackSparrow".parse::<Urn>().unwrap(),
                object_type: Some(NameBuf::new("Character").unwrap()),
                observed_at: None,
                dataset_id: None,
                instance_id: None,
                attributes: IndexMap::default(),
            }))),
        );
        let relationship = NgsiLdAttribute::Relationship(NgsiLdRelationship {
            object: "urn:ngsi-ld:Person:31".parse::<Urn>().unwrap(),
            object_type: Some(NameBuf::new("Person").unwrap()),
            observed_at: None,
            dataset_id: None,
            instance_id: None,
            attributes,
        });
        let entity = create_test_entity("001", "Movie", "hasLeadActor", relationship);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["hasLeadActor"]["object"], "urn:ngsi-ld:Person:31");
        assert_eq!(normalized["hasLeadActor"]["playsCharacter"]["type"], "Relationship");
        assert_eq!(normalized["hasLeadActor"]["playsCharacter"]["object"], "urn:ngsi-ld:Character:JackSparrow");
        assert_eq!(normalized["hasLeadActor"]["playsCharacter"]["objectType"], "Character");
    }

    #[test]
    fn test_list_relationship_with_metadata() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 0).unwrap();
        let list_rel = NgsiLdAttribute::ListRelationship(NgsiLdListRelationship {
            object_list: vec![
                "urn:ngsi-ld:Person:John".parse::<Urn>().unwrap(),
                "urn:ngsi-ld:Person:Paul".parse::<Urn>().unwrap(),
            ],
            object_type: Some(NameBuf::new("Person").unwrap()),
            observed_at: Some(dt),
            dataset_id: None,
            attributes: IndexMap::default(),
        });
        let entity = create_test_entity("001", "Group", "members", list_rel);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["members"]["type"], "ListRelationship");
        assert_eq!(normalized["members"]["objectType"], "Person");
        assert_eq!(normalized["members"]["observedAt"], "2023-12-25T10:30:00Z");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["members"]["objectType"], "Person");
        assert_eq!(concise["members"]["observedAt"], "2023-12-25T10:30:00Z");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert!(simplified["members"].is_array());
        assert_eq!(simplified["members"][0], "urn:ngsi-ld:Person:John");
        assert_eq!(simplified["members"][1], "urn:ngsi-ld:Person:Paul");
    }
}

mod multi_attribute_tests {
    use super::*;

    #[test]
    fn test_multi_property_all_modes() {
        let prop1 = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Number(20.into())),
            observed_at: None,
            unit_code: None,
            dataset_id: Some("urn:dataset:1".parse().unwrap()),
            instance_id: None,
            attributes: IndexMap::default(),
        });
        let prop2 = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Number(22.into())),
            observed_at: None,
            unit_code: None,
            dataset_id: Some("urn:dataset:2".parse().unwrap()),
            instance_id: None,
            attributes: IndexMap::default(),
        });

        let mut multi_attrs = IndexMap::default();
        multi_attrs.insert(NameBuf::new("temperature").unwrap(), NgsiLdAttributeWrapper::Multi(vec![prop1, prop2]));

        let multi_entity = NgsiLdEntity {
            id: "urn:ngsi-ld:Sensor:001".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("Sensor").unwrap(),
            context: Some(url_ctx("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld")),
            scope: None,
            attributes: multi_attrs,
        };

        let normalized = serialize_entity(&multi_entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert!(normalized["temperature"].is_array());
        assert_eq!(normalized["temperature"][0]["type"], "Property");
        assert_eq!(normalized["temperature"][0]["value"], 20);
        assert_eq!(normalized["temperature"][0]["datasetId"], "urn:dataset:1");
        assert_eq!(normalized["temperature"][1]["value"], 22);
        assert_eq!(normalized["temperature"][1]["datasetId"], "urn:dataset:2");

        let concise = serialize_entity(&multi_entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise["temperature"].is_array());
        assert_eq!(concise["temperature"][0]["value"], 20);
        assert_eq!(concise["temperature"][0]["datasetId"], "urn:dataset:1");

        let simplified = serialize_entity(&multi_entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert!(simplified["temperature"].is_array());
        assert_eq!(simplified["temperature"][0], 20);
        assert_eq!(simplified["temperature"][1], 22);
    }

    #[test]
    fn test_multi_relationship() {
        let rel1 = NgsiLdAttribute::Relationship(NgsiLdRelationship {
            object: "urn:ngsi-ld:Person:John".parse::<Urn>().unwrap(),
            object_type: None,
            observed_at: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });
        let rel2 = NgsiLdAttribute::Relationship(NgsiLdRelationship {
            object: "urn:ngsi-ld:Person:Paul".parse::<Urn>().unwrap(),
            object_type: None,
            observed_at: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });

        let mut multi_attrs = IndexMap::default();
        multi_attrs.insert(NameBuf::new("knows").unwrap(), NgsiLdAttributeWrapper::Multi(vec![rel1, rel2]));

        let multi_entity = NgsiLdEntity {
            id: "urn:ngsi-ld:Person:Ringo".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("Person").unwrap(),
            context: Some(url_ctx("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld")),
            scope: None,
            attributes: multi_attrs,
        };

        let normalized = serialize_entity(&multi_entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert!(normalized["knows"].is_array());
        assert_eq!(normalized["knows"][0]["type"], "Relationship");
        assert_eq!(normalized["knows"][0]["object"], "urn:ngsi-ld:Person:John");

        let concise = serialize_entity(&multi_entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise["knows"].is_array());
        assert_eq!(concise["knows"][0], "urn:ngsi-ld:Person:John");

        let simplified = serialize_entity(&multi_entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert!(simplified["knows"].is_array());
        assert_eq!(simplified["knows"][0], "urn:ngsi-ld:Person:John");
    }

    #[test]
    fn test_multi_relationship_carries_object_type_and_dataset_id_per_instance() {
        let departure = NgsiLdAttribute::Relationship(NgsiLdRelationship {
            object: "urn:ngsi-ld:Airport:535".parse::<Urn>().unwrap(),
            object_type: Some(NameBuf::new("Airport").unwrap()),
            observed_at: None,
            dataset_id: Some("urn:ngsi-ld:dataset:role:departure".parse().unwrap()),
            instance_id: None,
            attributes: IndexMap::default(),
        });
        let arrival = NgsiLdAttribute::Relationship(NgsiLdRelationship {
            object: "urn:ngsi-ld:Airport:340".parse::<Urn>().unwrap(),
            object_type: Some(NameBuf::new("Airport").unwrap()),
            observed_at: None,
            dataset_id: Some("urn:ngsi-ld:dataset:role:arrival".parse().unwrap()),
            instance_id: None,
            attributes: IndexMap::default(),
        });

        let mut multi_attrs = IndexMap::default();
        multi_attrs.insert(NameBuf::new("servesAirport").unwrap(), NgsiLdAttributeWrapper::Multi(vec![departure, arrival]));
        let entity = NgsiLdEntity {
            id: "urn:ngsi-ld:Flight:LH-EDI-FRA".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("Flight").unwrap(),
            context: Some(url_ctx("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld")),
            scope: None,
            attributes: multi_attrs,
        };

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert!(normalized["servesAirport"].is_array());
        assert_eq!(normalized["servesAirport"][0]["type"], "Relationship");
        assert_eq!(normalized["servesAirport"][0]["object"], "urn:ngsi-ld:Airport:535");
        assert_eq!(normalized["servesAirport"][0]["objectType"], "Airport");
        assert_eq!(normalized["servesAirport"][0]["datasetId"], "urn:ngsi-ld:dataset:role:departure");
        assert_eq!(normalized["servesAirport"][1]["object"], "urn:ngsi-ld:Airport:340");
        assert_eq!(normalized["servesAirport"][1]["datasetId"], "urn:ngsi-ld:dataset:role:arrival");

        // An instance carrying metadata cannot collapse to a bare URN in concise mode, so datasetId
        // and objectType survive.
        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise["servesAirport"].is_array());
        assert_eq!(concise["servesAirport"][0]["object"], "urn:ngsi-ld:Airport:535");
        assert_eq!(concise["servesAirport"][0]["objectType"], "Airport");
        assert_eq!(concise["servesAirport"][0]["datasetId"], "urn:ngsi-ld:dataset:role:departure");
        assert_eq!(concise["servesAirport"][1]["datasetId"], "urn:ngsi-ld:dataset:role:arrival");
    }

    #[test]
    fn test_multi_list_relationship_carries_object_list_and_dataset_id_per_instance() {
        let departure = NgsiLdAttribute::ListRelationship(NgsiLdListRelationship {
            object_list: vec!["urn:ngsi-ld:Airport:1".parse::<Urn>().unwrap(), "urn:ngsi-ld:Airport:2".parse::<Urn>().unwrap()],
            object_type: Some(NameBuf::new("Airport").unwrap()),
            observed_at: None,
            dataset_id: Some("urn:ngsi-ld:dataset:role:departure".parse().unwrap()),
            attributes: IndexMap::default(),
        });
        let arrival = NgsiLdAttribute::ListRelationship(NgsiLdListRelationship {
            object_list: vec!["urn:ngsi-ld:Airport:3".parse::<Urn>().unwrap()],
            object_type: Some(NameBuf::new("Airport").unwrap()),
            observed_at: None,
            dataset_id: Some("urn:ngsi-ld:dataset:role:arrival".parse().unwrap()),
            attributes: IndexMap::default(),
        });

        let mut multi_attrs = IndexMap::default();
        multi_attrs.insert(NameBuf::new("servesAirports").unwrap(), NgsiLdAttributeWrapper::Multi(vec![departure, arrival]));
        let entity = NgsiLdEntity {
            id: "urn:ngsi-ld:Route:1".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("Route").unwrap(),
            context: Some(url_ctx("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld")),
            scope: None,
            attributes: multi_attrs,
        };

        // Each instance is a full ListRelationship carrying its own objectList and datasetId (ETSI
        // GS CIM 009 v1.9.1 clause 4.5.5).
        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert!(normalized["servesAirports"].is_array());
        assert_eq!(normalized["servesAirports"][0]["type"], "ListRelationship");
        assert_eq!(normalized["servesAirports"][0]["objectList"][0], "urn:ngsi-ld:Airport:1");
        assert_eq!(normalized["servesAirports"][0]["objectList"][1], "urn:ngsi-ld:Airport:2");
        assert_eq!(normalized["servesAirports"][0]["objectType"], "Airport");
        assert_eq!(normalized["servesAirports"][0]["datasetId"], "urn:ngsi-ld:dataset:role:departure");
        assert_eq!(normalized["servesAirports"][1]["objectList"][0], "urn:ngsi-ld:Airport:3");
        assert_eq!(normalized["servesAirports"][1]["datasetId"], "urn:ngsi-ld:dataset:role:arrival");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise["servesAirports"].is_array());
        assert_eq!(concise["servesAirports"][0]["objectList"][0], "urn:ngsi-ld:Airport:1");
        assert_eq!(concise["servesAirports"][0]["datasetId"], "urn:ngsi-ld:dataset:role:departure");
        assert_eq!(concise["servesAirports"][1]["datasetId"], "urn:ngsi-ld:dataset:role:arrival");
    }
}

mod entity_list_tests {
    use super::*;

    #[test]
    fn test_entity_list_serialization() {
        let entity1 = create_test_entity(
            "001",
            "Person",
            "name",
            NgsiLdAttribute::Property(NgsiLdProperty {
                value: Value::from(JsonValue::String("John".to_string())),
                observed_at: None,
                unit_code: None,
                dataset_id: None,
                instance_id: None,
                attributes: IndexMap::default(),
            }),
        );

        let entity2 = create_test_entity(
            "002",
            "Person",
            "name",
            NgsiLdAttribute::Property(NgsiLdProperty {
                value: Value::from(JsonValue::String("Paul".to_string())),
                observed_at: None,
                unit_code: None,
                dataset_id: None,
                instance_id: None,
                attributes: IndexMap::default(),
            }),
        );

        let entities = vec![entity1, entity2];

        let normalized = serialize_entities(&entities, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert!(normalized.is_array());
        assert_eq!(normalized[0]["name"]["type"], "Property");
        assert_eq!(normalized[0]["name"]["value"], "John");
        assert_eq!(normalized[1]["name"]["value"], "Paul");

        let concise = serialize_entities(&entities, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise.is_array());
        assert_eq!(concise[0]["name"], "John");
        assert_eq!(concise[1]["name"], "Paul");

        let simplified = serialize_entities(&entities, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert!(simplified.is_array());
        assert_eq!(simplified[0]["name"], "John");
        assert_eq!(simplified[1]["name"], "Paul");
    }

    #[test]
    fn test_empty_entity_list() {
        let entities: Vec<NgsiLdEntity> = vec![];

        let normalized = serialize_entities(&entities, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert!(normalized.is_array());
        assert_eq!(normalized.as_array().unwrap().len(), 0);

        let concise = serialize_entities(&entities, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise.is_array());
        assert_eq!(concise.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_single_entity_in_list() {
        let entity = create_test_entity(
            "001",
            "Person",
            "name",
            NgsiLdAttribute::Property(NgsiLdProperty {
                value: Value::from(JsonValue::String("John".to_string())),
                observed_at: None,
                unit_code: None,
                dataset_id: None,
                instance_id: None,
                attributes: IndexMap::default(),
            }),
        );

        let entities = vec![entity];

        let result = serialize_entities(&entities, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap().len(), 1);
        assert_eq!(result[0]["name"]["type"], "Property");
    }
}

mod edge_case_tests {
    use super::*;
    use cefact_units::UnitCode;

    #[test]
    fn test_empty_properties() {
        let entity = NgsiLdEntity {
            id: "urn:ngsi-ld:Empty:001".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("Empty").unwrap(),
            context: None,
            scope: None,
            attributes: IndexMap::default(),
        };

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["id"], "urn:ngsi-ld:Empty:001");
        assert_eq!(normalized["type"], "Empty");
        assert!(!normalized.as_object().unwrap().contains_key("@context"));
        assert_eq!(normalized.as_object().unwrap().len(), 2);
    }

    #[test]
    fn test_all_attribute_types() {
        let mut attributes = IndexMap::default();

        attributes.insert(
            NameBuf::new("name").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty {
                value: Value::from(JsonValue::String("Test".to_string())),
                observed_at: None,
                unit_code: None,
                dataset_id: None,
                instance_id: None,
                attributes: IndexMap::default(),
            })),
        );

        attributes.insert(
            NameBuf::new("knows").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::Relationship(NgsiLdRelationship {
                object: "urn:ngsi-ld:Person:Other".parse::<Urn>().unwrap(),
                object_type: None,
                observed_at: None,
                dataset_id: None,
                instance_id: None,
                attributes: IndexMap::default(),
            })),
        );

        attributes.insert(
            NameBuf::new("location").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::GeoProperty(NgsiLdGeoProperty {
                value: NgsiLdGeometry::Point {
                    coordinates: [0.0, 0.0].into(),
                },
                observed_at: None,
                dataset_id: None,
                instance_id: None,
            })),
        );

        attributes.insert(
            NameBuf::new("friends").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::ListRelationship(NgsiLdListRelationship {
                object_list: vec!["urn:ngsi-ld:Person:A".parse::<Urn>().unwrap(), "urn:ngsi-ld:Person:B".parse::<Urn>().unwrap()],
                object_type: None,
                observed_at: None,
                dataset_id: None,
                attributes: IndexMap::default(),
            })),
        );

        let entity = NgsiLdEntity {
            id: "urn:ngsi-ld:AllTypes:001".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("AllTypes").unwrap(),
            context: Some(url_ctx("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld")),
            scope: None,
            attributes,
        };

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["name"]["type"], "Property");
        assert_eq!(normalized["knows"]["type"], "Relationship");
        assert_eq!(normalized["location"]["type"], "GeoProperty");
        assert_eq!(normalized["friends"]["type"], "ListRelationship");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["name"], "Test");
        assert_eq!(concise["knows"], "urn:ngsi-ld:Person:Other");
        assert_eq!(concise["location"]["type"], "Point");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["name"], "Test");
        assert_eq!(simplified["knows"], "urn:ngsi-ld:Person:Other");
        assert!(simplified["friends"].is_array());
    }

    #[test]
    fn test_complex_nested_properties() {
        let level3_prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Number(3.into())),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });

        let mut level2_props = IndexMap::default();
        level2_props.insert(NameBuf::new("level3").unwrap(), Box::new(NgsiLdAttributeWrapper::single(level3_prop)));

        let level2_prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Number(2.into())),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: level2_props,
        });

        let mut level1_props = IndexMap::default();
        level1_props.insert(NameBuf::new("level2").unwrap(), Box::new(NgsiLdAttributeWrapper::single(level2_prop)));

        let root_prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Number(1.into())),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: level1_props,
        });

        let entity = create_test_entity("001", "Nested", "root", root_prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["root"]["type"], "Property");
        assert_eq!(normalized["root"]["value"], 1);
        assert_eq!(normalized["root"]["level2"]["type"], "Property");
        assert_eq!(normalized["root"]["level2"]["value"], 2);
        assert_eq!(normalized["root"]["level2"]["level3"]["type"], "Property");
        assert_eq!(normalized["root"]["level2"]["level3"]["value"], 3);
    }

    #[test]
    fn test_unicode_and_special_characters() {
        let prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::String("🎵 John Lennon ♪".to_string())),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });
        // `NameBuf` only accepts `\p{L}[\p{L}\p{N}_]*`, so the attribute name itself stays plain
        // ASCII while its value carries the Unicode.
        let entity = create_test_entity("001", "Person", "name_unicode", prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["name_unicode"]["value"], "🎵 John Lennon ♪");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["name_unicode"], "🎵 John Lennon ♪");
    }

    #[test]
    fn test_large_numbers_and_precision() {
        let prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Number(serde_json::Number::from_f64(123_456_789.987_654_33).unwrap())),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });
        let entity = create_test_entity("001", "Test", "bigNumber", prop);

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        let actual_value = simplified["bigNumber"].as_f64().unwrap();
        let expected_value = 123_456_789.987_654_33;
        assert!((actual_value - expected_value).abs() < 1e-6, "Expected {expected_value}, got {actual_value}");
    }

    #[test]
    fn test_complex_geometry_serialization() {
        // A MultiPolygon is the most deeply nested geometry a GeoProperty may carry (ETSI GS CIM 009
        // v1.9.1 clause 4.7 with RFC 7946 clause 3.1.7); every representation must carry it whole.
        let multi_polygon = NgsiLdGeometry::MultiPolygon {
            coordinates: vec![
                vec![vec![
                    [100.0, 0.0].into(),
                    [101.0, 0.0].into(),
                    [101.0, 1.0].into(),
                    [100.0, 1.0].into(),
                    [100.0, 0.0].into(),
                ]],
                vec![vec![
                    [200.0, 0.0].into(),
                    [201.0, 0.0].into(),
                    [201.0, 1.0].into(),
                    [200.0, 1.0].into(),
                    [200.0, 0.0].into(),
                ]],
            ],
        };

        let geo = NgsiLdAttribute::GeoProperty(NgsiLdGeoProperty {
            value: multi_polygon,
            observed_at: Some("2023-01-01T00:00:00Z".parse().unwrap()),
            dataset_id: Some("urn:dataset:geo".parse().unwrap()),
            instance_id: None,
        });

        let entity = create_test_entity("001", "ComplexPlace", "complexLoc", geo);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["complexLoc"]["type"], "GeoProperty");
        assert_eq!(normalized["complexLoc"]["value"]["type"], "MultiPolygon");
        assert_eq!(normalized["complexLoc"]["value"]["coordinates"].as_array().unwrap().len(), 2);
        assert_eq!(normalized["complexLoc"]["observedAt"], "2023-01-01T00:00:00Z");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise["complexLoc"].is_object());
        assert_eq!(concise["complexLoc"]["value"]["type"], "MultiPolygon");
        assert_eq!(concise["complexLoc"]["value"]["coordinates"].as_array().unwrap().len(), 2);
        assert_eq!(concise["complexLoc"]["observedAt"], "2023-01-01T00:00:00Z");
        assert_eq!(concise["complexLoc"]["datasetId"], "urn:dataset:geo");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["complexLoc"]["type"], "MultiPolygon");
        assert_eq!(simplified["complexLoc"]["coordinates"].as_array().unwrap().len(), 2);
        assert_eq!(simplified["complexLoc"]["coordinates"][0][0][0][0], 100.0);
    }

    #[test]
    fn a_geometry_collection_cannot_be_read_as_a_geoproperty_value() {
        // ETSI GS CIM 009 v1.9.1 clause 4.7 admits six geometry types, and GeometryCollection
        // (RFC 7946 clause 3.1.8) is not among them, so it has no variant to deserialize into and a
        // GeoProperty carrying one is unrepresentable rather than merely invalid.
        let geometry_collection = json!({
            "type": "GeometryCollection",
            "geometries": [
                {"type": "Point", "coordinates": [100.0, 0.0]},
                {"type": "LineString", "coordinates": [[101.0, 0.0], [102.0, 1.0]]},
            ]
        });

        assert!(serde_json::from_value::<NgsiLdGeometry>(geometry_collection).is_err());
    }

    #[test]
    fn test_concise_preserves_unitcode_only() {
        let prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(json!(100)),
            observed_at: None,
            unit_code: Some(UnitCode::Kgm),
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });
        let entity = create_test_entity("001", "Weight", "mass", prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["mass"]["type"], "Property");
        assert_eq!(normalized["mass"]["value"], 100);
        assert_eq!(normalized["mass"]["unitCode"], "KGM");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise["mass"].is_object());
        assert_eq!(concise["mass"]["value"], 100);
        assert_eq!(concise["mass"]["unitCode"], "KGM");
        assert!(!concise["mass"].as_object().unwrap().contains_key("type"));

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["mass"], 100);
    }

    #[test]
    fn test_edge_case_empty_list_relationship() {
        let empty_list = NgsiLdAttribute::ListRelationship(NgsiLdListRelationship {
            object_list: vec![],
            object_type: None,
            observed_at: None,
            dataset_id: None,
            attributes: IndexMap::default(),
        });

        let entity = create_test_entity("001", "Test", "emptyList", empty_list);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Include);
        assert_eq!(normalized["emptyList"]["type"], "ListRelationship");
        assert_eq!(normalized["emptyList"]["objectList"].as_array().unwrap().len(), 0);

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Include);
        assert!(simplified["emptyList"].is_array());
        assert_eq!(simplified["emptyList"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_null_value_with_metadata_in_concise() {
        let prop_with_metadata = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Null),
            observed_at: Some("2023-12-25T10:30:00Z".parse().unwrap()),
            unit_code: Some(UnitCode::Kgm),
            dataset_id: Some("urn:dataset:sensor1".parse().unwrap()),
            instance_id: None,
            attributes: IndexMap::default(),
        });

        let entity = create_test_entity("001", "Sensor", "weight", prop_with_metadata);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Include);

        assert!(concise["weight"].is_object());
        assert!(concise["weight"]["value"].is_null());
        assert_eq!(concise["weight"]["unitCode"], "KGM");
        assert_eq!(concise["weight"]["observedAt"], "2023-12-25T10:30:00Z");
        assert_eq!(concise["weight"]["datasetId"], "urn:dataset:sensor1");
    }

    #[test]
    fn test_null_value_with_metadata_in_normalized() {
        let prop_with_metadata = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Null),
            observed_at: Some("2023-12-25T10:30:00Z".parse().unwrap()),
            unit_code: Some(UnitCode::Kgm),
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });

        let entity = create_test_entity("001", "Sensor", "weight", prop_with_metadata);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Include);

        assert_eq!(normalized["weight"]["type"], "Property");
        assert!(normalized["weight"]["value"].is_null());
        assert_eq!(normalized["weight"]["unitCode"], "KGM");
        assert_eq!(normalized["weight"]["observedAt"], "2023-12-25T10:30:00Z");
    }

    #[test]
    fn test_simple_null_value_in_concise() {
        let simple_null_prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(JsonValue::Null),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::default(),
        });

        let entity = create_test_entity("001", "Test", "simpleNull", simple_null_prop);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Include);

        assert!(concise["simpleNull"].is_null());
    }

    #[test]
    fn test_multi_wrapper_forces_array_even_if_single() {
        let prop = NgsiLdAttribute::Property(NgsiLdProperty {
            value: Value::from(json!(42)),
            observed_at: None,
            unit_code: None,
            dataset_id: Some("urn:dataset:1".parse().unwrap()),
            instance_id: None,
            attributes: IndexMap::default(),
        });

        let mut attrs = IndexMap::default();
        attrs.insert(NameBuf::new("forcedArray").unwrap(), NgsiLdAttributeWrapper::Multi(vec![prop]));

        let entity = NgsiLdEntity {
            id: "urn:ngsi-ld:Test:001".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("Test").unwrap(),
            context: None,
            scope: None,
            attributes: attrs,
        };

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);

        assert!(normalized["forcedArray"].is_array());
        let array = normalized["forcedArray"].as_array().unwrap();
        assert_eq!(array.len(), 1);
        assert_eq!(array[0]["value"], 42);
        assert_eq!(array[0]["datasetId"], "urn:dataset:1");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise["forcedArray"].is_array());
        assert_eq!(concise["forcedArray"].as_array().unwrap().len(), 1);
        assert_eq!(concise["forcedArray"][0]["value"], 42);

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert!(simplified["forcedArray"].is_array());
        assert_eq!(simplified["forcedArray"].as_array().unwrap().len(), 1);
        assert_eq!(simplified["forcedArray"][0], 42);
    }

    mod context_tests {
        use super::*;

        #[test]
        fn test_context_serialization() {
            let string_context = url_ctx("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld");
            let list_context = NgsiLdContext::List(vec![
                NgsiLdContextEntry::Remote(Url::parse("https://fiware.github.io/tutorials.Step-by-Step/example.jsonld").unwrap()),
                NgsiLdContextEntry::Remote(Url::parse("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld").unwrap()),
            ]);

            let mut map_context = IndexMap::default();
            map_context.insert("iot".into(), "https://uri.etsi.org/ngsi-ld/v1/iot-context.jsonld".to_string());

            let entity_with_string = NgsiLdEntity {
                id: "urn:ngsi-ld:Test:001".parse::<Urn>().unwrap(),
                entity_type: NameBuf::new("Test").unwrap(),
                context: Some(string_context),
                scope: None,
                attributes: IndexMap::default(),
            };

            let entity_with_list = NgsiLdEntity {
                id: "urn:ngsi-ld:Test:002".parse::<Urn>().unwrap(),
                entity_type: NameBuf::new("Test").unwrap(),
                context: Some(list_context),
                scope: None,
                attributes: IndexMap::default(),
            };

            let entity_with_map = NgsiLdEntity {
                id: "urn:ngsi-ld:Test:003".parse::<Urn>().unwrap(),
                entity_type: NameBuf::new("Test").unwrap(),
                context: Some(NgsiLdContext::Single(NgsiLdContextEntry::Inline(map_context))),
                scope: None,
                attributes: IndexMap::default(),
            };

            let entity_no_context = NgsiLdEntity {
                id: "urn:ngsi-ld:Test:004".parse::<Urn>().unwrap(),
                entity_type: NameBuf::new("Test").unwrap(),
                context: None,
                scope: None,
                attributes: IndexMap::default(),
            };

            let result1 = serialize_entity(&entity_with_string, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
            assert_eq!(result1["@context"], "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld");

            let result2 = serialize_entity(&entity_with_list, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
            assert!(result2["@context"].is_array());
            assert_eq!(result2["@context"][0], "https://fiware.github.io/tutorials.Step-by-Step/example.jsonld");

            let result3 = serialize_entity(&entity_with_map, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
            assert_eq!(result3["@context"]["iot"], "https://uri.etsi.org/ngsi-ld/v1/iot-context.jsonld");

            let result4 = serialize_entity(&entity_no_context, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
            assert!(!result4.as_object().unwrap().contains_key("@context"));
        }

        #[test]
        fn test_context_deserialization_with_mixed_array_entries() {
            let raw = json!([
                "https://raw.githubusercontent.com/smart-data-models/dataModel.Device/refs/heads/master/context.jsonld",
                {
                    "hasModel": "https://fiwarebox.com/dataModel#hasModel",
                    "headingName": "https://fiwarebox.com/dataModel#headingName"
                }
            ]);

            let parsed: NgsiLdContext = serde_json::from_value(raw).unwrap();

            match parsed {
                NgsiLdContext::List(entries) => {
                    assert_eq!(entries.len(), 2);
                    assert!(matches!(
                        &entries[0],
                        NgsiLdContextEntry::Remote(url)
                        if url.as_str() == "https://raw.githubusercontent.com/smart-data-models/dataModel.Device/refs/heads/master/context.jsonld"
                    ));
                    match &entries[1] {
                        NgsiLdContextEntry::Inline(map) => {
                            assert_eq!(map.get("hasModel").unwrap(), "https://fiwarebox.com/dataModel#hasModel");
                            assert_eq!(map.get("headingName").unwrap(), "https://fiwarebox.com/dataModel#headingName");
                        }
                        NgsiLdContextEntry::Remote(_) => {
                            panic!("expected inline entry in mixed @context array")
                        }
                    }
                }
                NgsiLdContext::Single(_) => {
                    panic!("expected list context for mixed @context array")
                }
            }
        }
    }

    mod property_ordering_tests {
        use super::*;

        #[test]
        fn test_ngsi_ld_property_ordering_all_modes() {
            let mut attributes = IndexMap::default();

            // Inserted out of alphabetical order, so the assertions below can tell insertion order
            // from alphabetical sorting.
            attributes.insert(
                NameBuf::new("a_attribute").unwrap(),
                NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty {
                    value: Value::from(JsonValue::String("first".to_string())),
                    observed_at: None,
                    unit_code: None,
                    dataset_id: None,
                    instance_id: None,
                    attributes: IndexMap::default(),
                })),
            );

            attributes.insert(
                NameBuf::new("z_attribute").unwrap(),
                NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty {
                    value: Value::from(JsonValue::String("last".to_string())),
                    observed_at: None,
                    unit_code: None,
                    dataset_id: None,
                    instance_id: None,
                    attributes: IndexMap::default(),
                })),
            );

            attributes.insert(
                NameBuf::new("m_attribute").unwrap(),
                NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty {
                    value: Value::from(JsonValue::String("middle".to_string())),
                    observed_at: None,
                    unit_code: None,
                    dataset_id: None,
                    instance_id: None,
                    attributes: IndexMap::default(),
                })),
            );

            let entity = NgsiLdEntity {
                id: "urn:ngsi-ld:Test:ordering".parse().unwrap(),
                entity_type: NameBuf::new("TestEntity").unwrap(),
                context: Some(url_ctx("https://schema.org")),
                scope: None,
                attributes,
            };

            let representations = vec![
                NgsiLdRepresentation::Normalized,
                NgsiLdRepresentation::Concise,
                NgsiLdRepresentation::Simplified,
            ];

            for repr in representations {
                let serialized = entity.to_string(repr, NgsiLdSkipNull::Include, JsonLayout::Compact).unwrap();

                let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
                let obj = parsed.as_object().unwrap();

                let keys: Vec<&str> = obj.keys().map(String::as_str).collect();

                assert_eq!(keys[0], "@context", "First property should be '@context' in {repr:?} mode");
                assert_eq!(keys[1], "id", "Second property should be 'id' in {repr:?} mode");
                assert_eq!(keys[2], "type", "Third property should be 'type' in {repr:?} mode");

                assert_eq!(keys[3], "a_attribute", "First attribute should maintain insertion order in {repr:?} mode");
                assert_eq!(keys[4], "z_attribute", "Second attribute should maintain insertion order in {repr:?} mode");
                assert_eq!(keys[5], "m_attribute", "Third attribute should maintain insertion order in {repr:?} mode");

                assert_eq!(keys.len(), 6, "Should have exactly 6 properties in {repr:?} mode");
            }
        }
    }

    mod vocab_property_tests {
        use super::*;

        #[test]
        fn test_vocab_property_all_modes() {
            let vocab = NgsiLdAttribute::VocabProperty(NgsiLdVocabProperty {
                has_vocab: IriBuf::new("urn:ngsi-ld:vocab:electricMicrobus").unwrap(),
                observed_at: None,
                dataset_id: None,
                attributes: IndexMap::default(),
            });
            let entity = create_test_entity("001", "Vehicle", "vehicleType", vocab);

            let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
            assert_eq!(normalized["vehicleType"]["type"], "VocabProperty");
            assert_eq!(normalized["vehicleType"]["vocab"], "urn:ngsi-ld:vocab:electricMicrobus");

            let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
            assert_eq!(concise["vehicleType"], "urn:ngsi-ld:vocab:electricMicrobus");

            let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
            assert_eq!(simplified["vehicleType"], "urn:ngsi-ld:vocab:electricMicrobus");
        }

        #[test]
        fn test_vocab_property_with_metadata() {
            let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 0).unwrap();
            let vocab = NgsiLdAttribute::VocabProperty(NgsiLdVocabProperty {
                has_vocab: IriBuf::new("urn:ngsi-ld:vocab:electricMicrobus").unwrap(),
                observed_at: Some(dt),
                dataset_id: Some("urn:dataset:vocab".parse().unwrap()),
                attributes: IndexMap::default(),
            });
            let entity = create_test_entity("001", "Vehicle", "vehicleType", vocab);

            let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
            assert_eq!(concise["vehicleType"]["vocab"], "urn:ngsi-ld:vocab:electricMicrobus");
            assert_eq!(concise["vehicleType"]["observedAt"], "2023-12-25T10:30:00Z");
        }
    }

    mod list_property_tests {
        use super::*;

        #[test]
        fn test_list_property_all_modes() {
            let list = NgsiLdAttribute::ListProperty(NgsiLdListProperty {
                has_value_list: vec![Value::from(json!(124)), Value::from(json!(156))],
                observed_at: None,
                dataset_id: None,
                attributes: IndexMap::default(),
            });
            let entity = create_test_entity("001", "TrafficFlowObserved", "hourlyVehicleCounts", list);

            let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
            assert_eq!(normalized["hourlyVehicleCounts"]["type"], "ListProperty");
            assert_eq!(normalized["hourlyVehicleCounts"]["valueList"], json!([124, 156]));

            let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
            assert_eq!(concise["hourlyVehicleCounts"], json!([124, 156]));

            let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
            assert_eq!(simplified["hourlyVehicleCounts"], json!([124, 156]));
        }
    }

    mod json_property_tests {
        use super::*;

        #[test]
        fn test_json_property_all_modes() {
            let json_val = json!({
                "uptime_hrs": 4102,
                "battery_mv": 3200
            });
            let json_prop = NgsiLdAttribute::JsonProperty(NgsiLdJsonProperty {
                has_json: Value::from(json_val.clone()),
                observed_at: None,
                dataset_id: None,
                attributes: IndexMap::default(),
            });
            let entity = create_test_entity("001", "ParkingSpot", "sensorDiagnostics", json_prop);

            let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
            assert_eq!(normalized["sensorDiagnostics"]["type"], "JsonProperty");
            assert_eq!(normalized["sensorDiagnostics"]["json"], json_val);

            let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
            assert_eq!(concise["sensorDiagnostics"], json_val);

            let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
            assert_eq!(simplified["sensorDiagnostics"], json_val);
        }
    }
}

#[test]
fn an_entity_round_trips_through_serde_keeping_its_attributes_in_declaration_order() {
    // The attributes are `#[serde(flatten)]`ed into the top-level object; `indexmap`'s serde impls are
    // generic over the hasher, so both halves must survive it, order included.
    let mut attributes = Attributes::default();
    for key in ["zone", "humidity", "airTemperature"] {
        attributes.insert(
            NameBuf::new(key).unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(Value::from(json!(key))))),
        );
    }
    let entity = NgsiLdEntity {
        context: None,
        id: "urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap(),
        entity_type: NameBuf::new("Sensor").unwrap(),
        scope: None,
        attributes,
    };

    let encoded = serde_json::to_string(&entity).unwrap();
    let decoded: NgsiLdEntity = serde_json::from_str(&encoded).unwrap();

    assert_eq!(
        decoded.attributes.keys().map(NameBuf::as_str).collect::<Vec<&str>>(),
        ["zone", "humidity", "airTemperature"]
    );
    assert_eq!(decoded, entity);
}

#[test]
fn attributes_are_reachable_and_removable_by_a_plain_string_key() {
    // `NameBuf: Borrow<str>` is what lets the pipeline probe by `&str`; the impl is hash-independent,
    // so it must keep working under the swapped hasher.
    let mut attributes = Attributes::default();
    attributes.insert(
        NameBuf::new("observedAt").unwrap(),
        NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(Value::from(json!("2026-04-03T22:00:20Z"))))),
    );

    assert!(attributes.get("observedAt").is_some());
    assert!(attributes.swap_remove("observedAt").is_some());
    assert!(attributes.get("observedAt").is_none());
}
