use crate::{
    attribute_kind::classify_attribute_kind,
    constraint::node_constraints,
    editor::EditorAttribute,
    schema_node_details::SchemaNodeDetails,
    screen::wizard::{editor_key::EditorKey, node_path::NodePath},
};
use cassiopeia_mapping::transformation::Transformation;
use cassiopeia_ngsi_ld::entity::{attribute::NgsiLdAttributeKind, reserved_member::is_reserved_member};
use indexmap::IndexMap;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// A Smart Data Model JSON Schema parsed into an editable attribute tree.
pub struct ParsedSchema {
    /// The top-level attributes, keyed by their editing-tree key.
    pub attributes: IndexMap<EditorKey, EditorAttribute>,

    /// The names the schema marks required at the top level.
    pub required: HashSet<String>,

    /// The detail facts for every node, keyed by its path from the root.
    pub details: HashMap<NodePath, SchemaNodeDetails>,
}

/// Parses a dereferenced Smart Data Model schema into an editable tree of attributes.
#[must_use]
pub fn parse_schema(schema: &Value) -> ParsedSchema {
    let mut attributes = IndexMap::new();
    let mut required = HashSet::new();
    let mut details = HashMap::new();

    parse_node_recursive(schema, &mut attributes, &mut required, &mut details, &NodePath::new());

    ParsedSchema { attributes, required, details }
}

fn parse_node_recursive(
    schema: &Value,
    map: &mut IndexMap<EditorKey, EditorAttribute>,
    req_set: &mut HashSet<String>,
    details_map: &mut HashMap<NodePath, SchemaNodeDetails>,
    path: &NodePath,
) {
    if !path.is_empty() {
        let description = schema
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or("No description provided.")
            .replace("Property. ", "")
            .replace("Relationship. ", "")
            .replace("Model:", "\nModel:");

        let data_type = schema.get("type").and_then(|value| value.as_str()).unwrap_or("Mixed/Complex").to_string();

        details_map.insert(
            path.clone(),
            SchemaNodeDetails {
                description,
                data_type,
                required: false,
                constraints: node_constraints(schema),
            },
        );
    }

    if let Some(requirements) = schema.get("required").and_then(|value| value.as_array()) {
        for requirement in requirements {
            if let Some(name) = requirement.as_str()
                && path.is_empty()
            {
                req_set.insert(name.to_string());
            }
        }
    }

    if let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) {
        for (key, sub_schema) in properties {
            if is_reserved_member(key) {
                continue;
            }

            if let Some(one_of) = sub_schema.get("oneOf").and_then(|value| value.as_array()) {
                let mut parent_attr = schema_to_attribute(sub_schema);
                parent_attr.attribute_type = NgsiLdAttributeKind::Property;
                parent_attr.transformation = Some(Transformation::Object);

                let parent_path = path.child(EditorKey::SchemaField(key.clone()));
                parse_node_recursive(sub_schema, &mut IndexMap::new(), &mut HashSet::new(), details_map, &parent_path);

                let mut sub_map = IndexMap::new();
                for (index, option) in one_of.iter().enumerate() {
                    let position = u32::try_from(index + 1).unwrap_or(u32::MAX);
                    let title = option
                        .get("title")
                        .and_then(|value| value.as_str())
                        .map_or_else(|| format!("Choice {position}"), str::to_string);
                    let option_key = EditorKey::OneOfOption { index: position, title };

                    let mut attr = schema_to_attribute(option);

                    if let Some(title) = option.get("title").and_then(|value| value.as_str()) {
                        if title.contains("Point") || title.contains("LineString") || title.contains("Polygon") || title.contains("Multi") {
                            attr.attribute_type = NgsiLdAttributeKind::GeoProperty;
                        } else {
                            let description = option.get("description").and_then(|value| value.as_str()).unwrap_or("");
                            if description.contains("Relationship") {
                                attr.attribute_type = NgsiLdAttributeKind::Relationship;
                            }
                        }
                    }

                    let new_path = parent_path.child(option_key.clone());

                    if attr.is_container() {
                        let mut nested_map = IndexMap::new();
                        let mut nested_req = HashSet::new();
                        parse_node_recursive(option, &mut nested_map, &mut nested_req, details_map, &new_path);
                        attr.mappings = nested_map;
                    } else {
                        parse_node_recursive(option, &mut IndexMap::new(), &mut HashSet::new(), details_map, &new_path);
                    }

                    sub_map.insert(option_key, attr);
                }

                parent_attr.mappings = sub_map;
                map.insert(EditorKey::SchemaField(key.clone()), parent_attr);
            } else {
                let mut attr = schema_to_attribute(sub_schema);

                let child_key = EditorKey::SchemaField(key.clone());
                let new_path = path.child(child_key.clone());

                if attr.is_container() {
                    let mut sub_map = IndexMap::new();
                    let mut sub_req = HashSet::new();
                    parse_node_recursive(sub_schema, &mut sub_map, &mut sub_req, details_map, &new_path);
                    attr.mappings = sub_map;
                } else {
                    parse_node_recursive(sub_schema, &mut IndexMap::new(), &mut HashSet::new(), details_map, &new_path);
                }

                map.insert(child_key, attr);
            }
        }
    }

    if let Some(all_of) = schema.get("allOf").and_then(|value| value.as_array()) {
        for sub in all_of {
            parse_node_recursive(sub, map, req_set, details_map, path);
        }
    }
}

fn schema_to_attribute(schema: &Value) -> EditorAttribute {
    let json_type = schema.get("type").and_then(|value| value.as_str());
    let attribute_type = classify_attribute_kind(schema);

    let transformation = match json_type {
        Some("string") => match schema.get("format").and_then(|value| value.as_str()) {
            Some("date-time") => Some(Transformation::DateTime),
            Some("date") => Some(Transformation::Date),
            Some("time") => Some(Transformation::Time),
            // `format` is an open string vocabulary; any other value is a plain string.
            _ => Some(Transformation::String),
        },
        Some("number") => Some(Transformation::Float),
        Some("integer") => Some(Transformation::Integer),
        Some("boolean") => Some(Transformation::Boolean),
        Some("array") => Some(Transformation::Array),
        Some("object") => Some(Transformation::Object),
        // `type` is an open string vocabulary; anything else carries no conversion.
        _ => None,
    };

    if let Some(title) = schema.get("title").and_then(|value| value.as_str()) {
        let geometry = match title {
            "GeoJSON Point" => Some(Transformation::Point),
            "GeoJSON MultiPoint" => Some(Transformation::MultiPoint),
            "GeoJSON LineString" => Some(Transformation::LineString),
            "GeoJSON MultiLineString" => Some(Transformation::MultiLineString),
            "GeoJSON Polygon" => Some(Transformation::Polygon),
            "GeoJSON MultiPolygon" => Some(Transformation::MultiPolygon),
            _ if attribute_type == NgsiLdAttributeKind::GeoProperty => Some(geojson_default_transformation()),
            // Any other title leaves the JSON-type-derived conversion in place.
            _ => transformation,
        };

        return EditorAttribute::new(attribute_type, geometry);
    }

    let final_transform = if attribute_type == NgsiLdAttributeKind::GeoProperty {
        Some(geojson_default_transformation())
    } else {
        transformation
    };

    EditorAttribute::new(attribute_type, final_transform)
}

/// The default geometry conversion for a `GeoProperty` whose specific geometry is not stated by a
/// title. The wizard lets the user narrow it afterwards.
const fn geojson_default_transformation() -> Transformation {
    Transformation::Point
}

#[cfg(test)]
mod tests {
    use crate::{schema_parser::parse_schema, screen::wizard::editor_key::EditorKey};
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
    use serde_json::json;

    fn field(name: &str) -> EditorKey {
        EditorKey::SchemaField(name.to_string())
    }

    #[test]
    fn required_top_level_fields_are_recorded() {
        let schema = json!({
            "required": ["temperature"],
            "properties": {"temperature": {"type": "number"}},
        });

        let parsed = parse_schema(&schema);

        assert!(parsed.required.contains("temperature"));
    }

    #[test]
    fn identity_fields_are_skipped() {
        let schema = json!({"properties": {"id": {"type": "string"}, "type": {"type": "string"}, "name": {"type": "string"}}});

        let parsed = parse_schema(&schema);

        assert!(!parsed.attributes.contains_key(&field("id")));
        assert!(!parsed.attributes.contains_key(&field("type")));
        assert!(parsed.attributes.contains_key(&field("name")));
    }

    #[test]
    fn a_relationship_is_detected_from_its_description() {
        let schema = json!({"properties": {"refRoad": {"type": "string", "description": "Relationship. Points at a road."}}});

        let parsed = parse_schema(&schema);

        assert_eq!(
            parsed.attributes.get(&field("refRoad")).unwrap().attribute_type,
            NgsiLdAttributeKind::Relationship
        );
    }

    #[test]
    fn a_oneof_property_becomes_an_object_container_with_one_typed_option_per_choice() {
        let schema = json!({
            "properties": {
                "location": {
                    "oneOf": [
                        {"title": "GeoJSON Point"},
                        {"title": "GeoJSON Polygon"},
                    ],
                },
            },
        });

        let parsed = parse_schema(&schema);
        let container = parsed.attributes.get(&field("location")).unwrap();

        assert!(container.is_container());
        assert_eq!(container.mappings.len(), 2);
        assert!(container.mappings.keys().all(EditorKey::is_one_of_option));
        assert_eq!(container.mappings.keys().next().unwrap().to_string(), "Option 1 - GeoJSON Point");
    }
}
