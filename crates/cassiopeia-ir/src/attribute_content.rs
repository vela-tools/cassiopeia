use cassiopeia_ngsi_ld::{
    entity::attribute::language_property::LanguageMap,
    value::{json_payload::JsonPayload, types::Value},
};
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// What one attribute instance resolved to, before it is built into an NGSI-LD attribute.
///
/// A `JsonProperty` carries raw JSON that NGSI-LD never interprets (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.24.2), in which a null is kept as the source wrote it. A `VocabProperty` carries the IRI its
/// `vocab` member names (clause 4.5.20), and a `LanguageProperty` the `languageMap` of text keyed by
/// language tag (clause 4.5.18): extraction refuses a value either kind cannot hold, so neither can
/// reach the transformer as something it would have to drop. Every other kind carries an NGSI-LD
/// Value, in which no null can sit (clause 4.5.0). Each travels apart so none can stand in for
/// another.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AttributeContent {
    /// The NGSI-LD Value of a Property, one of its subclasses not listed below, or a Relationship's
    /// object.
    Value(Value),
    /// The raw JSON of a `JsonProperty`.
    Json(JsonPayload),
    /// The vocabulary IRI of a `VocabProperty`.
    Vocab(IriBuf),
    /// The `languageMap` of a `LanguageProperty`, never empty.
    Language(LanguageMap),
}

impl AttributeContent {
    /// The NGSI-LD Value, when this is one rather than content typed for a Property subclass.
    #[must_use]
    pub const fn as_value(&self) -> Option<&Value> {
        match self {
            AttributeContent::Value(value) => Some(value),
            AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_) => None,
        }
    }
}

/// Writes content as the raw JSON it stands for inside a structured value, such as a member of an
/// object nested `mappings` assemble.
///
/// A Value is the JSON it serializes to, raw JSON is kept as it is, nulls included (ETSI GS CIM 009
/// v1.9.1 clause 4.5.24.2), a vocabulary IRI is its text, and a `languageMap` is the object of text
/// keyed by language tag it is (clause 4.5.18). The structured value that holds the result is
/// admitted by its own attribute's type, which decides whether a null in it survives.
// Manual because the content unwraps into the JSON of what it holds rather than the tagged form its
// `Serialize` writes, which no derive expresses.
impl From<AttributeContent> for JsonValue {
    fn from(content: AttributeContent) -> JsonValue {
        match content {
            AttributeContent::Value(value) => JsonValue::from(value),
            AttributeContent::Json(payload) => JsonValue::from(payload),
            AttributeContent::Vocab(iri) => JsonValue::String(iri.into_inner()),
            AttributeContent::Language(map) => JsonValue::Object(
                map.into_iter()
                    .map(|(language, text)| (language.into_string(), JsonValue::String(text)))
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_content::AttributeContent;
    use cassiopeia_ngsi_ld::{
        entity::attribute::language_property::LanguageMap,
        value::{json_payload::JsonPayload, types::Value},
    };
    use iri_rs::IriBuf;
    use langtag::LangTagBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

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
    fn vocab_and_language_content_expose_no_value() {
        let vocab = AttributeContent::Vocab(IriBuf::new("https://example.org/level/high".to_owned()).unwrap());
        let mut map = LanguageMap::default();
        map.insert(LangTagBuf::new("en".to_owned()).unwrap(), "hello".to_owned());

        assert_eq!(vocab.as_value(), None);
        assert_eq!(AttributeContent::Language(map).as_value(), None);
    }

    #[test]
    fn a_json_content_round_trips_through_json_keeping_its_nested_null() {
        let content = AttributeContent::Json(JsonPayload::try_from(json!({"maybe": null})).unwrap());
        let decoded: AttributeContent = serde_json::from_str(&serde_json::to_string(&content).unwrap()).unwrap();

        assert_eq!(decoded, content);
    }

    #[test]
    fn vocab_and_language_content_round_trip_through_json() {
        let mut map = LanguageMap::default();
        map.insert(LangTagBuf::new("pt-BR".to_owned()).unwrap(), "olá".to_owned());
        for content in [
            AttributeContent::Vocab(IriBuf::new("https://example.org/level/high".to_owned()).unwrap()),
            AttributeContent::Language(map),
        ] {
            let decoded: AttributeContent = serde_json::from_str(&serde_json::to_string(&content).unwrap()).unwrap();

            assert_eq!(decoded, content);
        }
    }

    #[test]
    fn each_content_writes_as_the_raw_json_it_stands_for() {
        let mut map = LanguageMap::default();
        map.insert(LangTagBuf::new("en".to_owned()).unwrap(), "hello".to_owned());

        assert_eq!(JsonValue::from(AttributeContent::Value(Value::Boolean(true))), json!(true));
        assert_eq!(
            JsonValue::from(AttributeContent::Json(JsonPayload::try_from(json!({"maybe": null})).unwrap())),
            json!({"maybe": null})
        );
        assert_eq!(
            JsonValue::from(AttributeContent::Vocab(IriBuf::new("https://example.org/level/high".to_owned()).unwrap())),
            json!("https://example.org/level/high")
        );
        assert_eq!(JsonValue::from(AttributeContent::Language(map)), json!({"en": "hello"}));
    }
}
