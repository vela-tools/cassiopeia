use cassiopeia_ngsi_ld::value::{json_payload::JsonPayload, types::Value};
use serde::{Deserialize, Serialize};

/// What one attribute instance resolved to, before it is built into an NGSI-LD attribute.
///
/// A `JsonProperty` carries raw JSON that NGSI-LD never interprets (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.24.2), in which a null is kept as the source wrote it; every other kind carries an NGSI-LD
/// Value, in which no null can sit (clause 4.5.0). The two travel apart so neither can stand in for
/// the other.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AttributeContent {
    /// The NGSI-LD Value of a Property, one of its subclasses other than `JsonProperty`, or a
    /// Relationship's object.
    Value(Value),
    /// The raw JSON of a `JsonProperty`.
    Json(JsonPayload),
}

impl AttributeContent {
    /// The NGSI-LD Value, when this is one rather than raw JSON.
    #[must_use]
    pub const fn as_value(&self) -> Option<&Value> {
        match self {
            AttributeContent::Value(value) => Some(value),
            AttributeContent::Json(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_content::AttributeContent;
    use cassiopeia_ngsi_ld::value::{json_payload::JsonPayload, types::Value};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn a_value_content_exposes_its_value() {
        assert_eq!(AttributeContent::Value(Value::Boolean(true)).as_value(), Some(&Value::Boolean(true)));
    }

    #[test]
    fn a_json_content_exposes_no_value() {
        let content = AttributeContent::Json(JsonPayload::try_from(json!({"maybe": null})).unwrap());

        assert_eq!(content.as_value(), None);
    }

    #[test]
    fn a_json_content_round_trips_through_json_keeping_its_nested_null() {
        let content = AttributeContent::Json(JsonPayload::try_from(json!({"maybe": null})).unwrap());
        let decoded: AttributeContent = serde_json::from_str(&serde_json::to_string(&content).unwrap()).unwrap();

        assert_eq!(decoded, content);
    }
}
