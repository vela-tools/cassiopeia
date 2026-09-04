use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
use serde_json::Value;

/// Infers the NGSI-LD attribute kind a Smart Data Model schema node describes.
///
/// Smart Data Models encode the intended attribute kind in the node's `description` prefix
/// (`Relationship. ...`, `GeoProperty. ...`, and so on) and its JSON type; this reads that
/// vocabulary and returns the typed kind. A node that matches none of the specific kinds is a plain
/// `Property`.
#[must_use]
pub fn classify_attribute_kind(schema: &Value) -> NgsiLdAttributeKind {
    let description = schema.get("description").and_then(|value| value.as_str()).unwrap_or("");
    let json_type = schema.get("type").and_then(|value| value.as_str());

    if json_type == Some("array") && has_relationship_array_pattern(schema) {
        NgsiLdAttributeKind::ListRelationship
    } else if description.contains("Relationship") {
        NgsiLdAttributeKind::Relationship
    } else if description.contains("GeoProperty") || (json_type == Some("object") && schema.pointer("/properties/coordinates").is_some()) {
        NgsiLdAttributeKind::GeoProperty
    } else if description.contains("LanguageProperty") {
        NgsiLdAttributeKind::LanguageProperty
    } else if description.contains("VocabProperty") {
        NgsiLdAttributeKind::VocabProperty
    } else if description.contains("ListProperty") {
        NgsiLdAttributeKind::ListProperty
    } else if description.contains("JsonProperty") {
        NgsiLdAttributeKind::JsonProperty
    } else {
        NgsiLdAttributeKind::Property
    }
}

/// Whether an array node's items all describe relationship targets, marking it a `ListRelationship`.
fn has_relationship_array_pattern(schema: &Value) -> bool {
    let Some(any_of) = schema.get("items").and_then(|items| items.get("anyOf")).and_then(|value| value.as_array()) else {
        return false;
    };

    any_of.iter().all(|item| {
        let describes_relationship = item
            .get("description")
            .and_then(|value| value.as_str())
            .is_some_and(|description| description.contains("Relationship"));

        let has_entity_pattern = item.get("pattern").and_then(|value| value.as_str()).is_some_and(|pattern| {
            pattern.contains("^[\\w\\-\\.\\{\\}\\$\\+\\*\\[\\]`|~^!,:\\\\]+$") || pattern.contains("entity") || pattern.contains("NGSI")
        });

        let has_uri_format = item.get("format").and_then(|value| value.as_str()) == Some("uri");

        let has_entity_length_pattern = item.get("maxLength").and_then(Value::as_u64) == Some(256) && item.get("minLength").and_then(Value::as_u64) == Some(1);

        describes_relationship || has_entity_pattern || has_uri_format || has_entity_length_pattern
    })
}

#[cfg(test)]
mod tests {
    use crate::attribute_kind::classify_attribute_kind;
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
    use serde_json::json;

    #[test]
    fn a_relationship_is_detected_from_its_description() {
        let schema = json!({"type": "string", "description": "Relationship. Points at a road."});
        assert_eq!(classify_attribute_kind(&schema), NgsiLdAttributeKind::Relationship);
    }

    #[test]
    fn a_geoproperty_is_detected_from_a_coordinates_object() {
        let schema = json!({"type": "object", "properties": {"coordinates": {"type": "array"}}});
        assert_eq!(classify_attribute_kind(&schema), NgsiLdAttributeKind::GeoProperty);
    }

    #[test]
    fn a_plain_string_is_a_property() {
        assert_eq!(classify_attribute_kind(&json!({"type": "string"})), NgsiLdAttributeKind::Property);
    }

    #[test]
    fn an_array_of_relationship_items_is_a_list_relationship() {
        let schema = json!({
            "type": "array",
            "items": {"anyOf": [{"format": "uri"}]},
        });
        assert_eq!(classify_attribute_kind(&schema), NgsiLdAttributeKind::ListRelationship);
    }
}
