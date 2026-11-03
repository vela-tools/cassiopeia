use crate::{attribute::refusal::AttributeRefusal, required_value::RequiredValue};
use cassiopeia_ir::attribute_content::AttributeContent;
use cassiopeia_ngsi_ld::{
    entity::attribute::{NgsiLdAttributeKind, language_property::LanguageMap},
    value::{json_payload::JsonPayload, types::Value},
};
use iri_rs::IriBuf;
use langtag::LangTagBuf;

/// Types a declaration's converted content as the NGSI-LD kind it declared, for a kind whose value
/// is not an NGSI-LD Value.
///
/// A `JsonProperty` holds raw JSON (ETSI GS CIM 009 v1.9.1 clause 4.5.24.2), a `VocabProperty` a
/// vocabulary IRI (clause 4.5.20), and a `LanguageProperty` a `languageMap` (clause 4.5.18). Typing
/// them here, where a refusal can still be recorded, means the transformer is handed only content
/// it can build, so none of these kinds can be lost without the run saying so. Every other kind, and
/// content already typed, passes through unchanged.
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming what the kind could not hold.
pub(crate) fn type_for_kind(kind: NgsiLdAttributeKind, content: AttributeContent) -> Result<Option<AttributeContent>, AttributeRefusal> {
    match (kind, content) {
        (NgsiLdAttributeKind::JsonProperty, AttributeContent::Value(value)) => Ok(Some(AttributeContent::Json(JsonPayload::try_from(value)?))),
        (NgsiLdAttributeKind::VocabProperty, AttributeContent::Value(value)) => vocab(&value),
        (NgsiLdAttributeKind::LanguageProperty, AttributeContent::Value(value)) => language_map(&value),
        (
            NgsiLdAttributeKind::Property
            | NgsiLdAttributeKind::GeoProperty
            | NgsiLdAttributeKind::ListProperty
            | NgsiLdAttributeKind::Relationship
            | NgsiLdAttributeKind::ListRelationship,
            content,
        )
        | (
            NgsiLdAttributeKind::JsonProperty | NgsiLdAttributeKind::VocabProperty | NgsiLdAttributeKind::LanguageProperty,
            content @ (AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_)),
        ) => Ok(Some(content)),
    }
}

/// Reads a value as the IRI a `VocabProperty` names.
///
/// A blank value is one the record does not carry, so it yields no content rather than a refusal.
/// The IRI is read from the value's text without the whitespace around it.
fn vocab(value: &Value) -> Result<Option<AttributeContent>, AttributeRefusal> {
    let Some(text) = value.reported_text() else {
        return Ok(None);
    };

    IriBuf::new(text.to_string())
        .map(|iri| Some(AttributeContent::Vocab(iri)))
        .map_err(|_| unusable(RequiredValue::Iri, &text))
}

/// Reads a value as the `languageMap` of a `LanguageProperty`.
///
/// Only an object can be one, and only its entries keyed by a well-formed BCP-47 tag and holding
/// text enter the map. An object with no such entry is refused like any other value: an empty
/// `languageMap` carries no value at all. A blank value is one the record does not carry, so it
/// yields no content rather than a refusal.
fn language_map(value: &Value) -> Result<Option<AttributeContent>, AttributeRefusal> {
    let Some(text) = value.reported_text() else {
        return Ok(None);
    };

    let map = match value {
        Value::Object(entries) => entries
            .iter()
            .filter_map(|(language, text)| Some((LangTagBuf::new(language.to_string()).ok()?, text.as_str()?.to_owned())))
            .collect(),
        Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) => LanguageMap::default(),
    };

    if map.is_empty() {
        Err(unusable(RequiredValue::LanguageMap, &text))
    } else {
        Ok(Some(AttributeContent::Language(map)))
    }
}

/// The refusal of a value that is not the `required` one, quoting the text it was met with.
fn unusable(required: RequiredValue, text: &str) -> AttributeRefusal {
    AttributeRefusal::UnusableValue {
        required,
        text: Box::from(text),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{kind_content::type_for_kind, refusal::AttributeRefusal},
        required_value::RequiredValue,
    };
    use cassiopeia_ir::attribute_content::AttributeContent;
    use cassiopeia_ngsi_ld::{
        entity::attribute::{NgsiLdAttributeKind, language_property::LanguageMap},
        value::{
            json_payload::JsonPayload,
            json_payload_error::{JsonPayloadError, JsonShape},
            types::Value,
        },
    };
    use iri_rs::IriBuf;
    use langtag::LangTagBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    fn value(json: JsonValue) -> AttributeContent {
        AttributeContent::Value(Value::try_from(json).unwrap())
    }

    fn unusable(required: RequiredValue, text: &str) -> AttributeRefusal {
        AttributeRefusal::UnusableValue {
            required,
            text: Box::from(text),
        }
    }

    fn language_map(entries: &[(&str, &str)]) -> LanguageMap {
        entries
            .iter()
            .map(|(tag, text)| (LangTagBuf::new((*tag).to_owned()).unwrap(), (*text).to_owned()))
            .collect()
    }

    #[test]
    fn an_iri_becomes_vocab_content_without_the_whitespace_around_it() {
        assert_eq!(
            type_for_kind(NgsiLdAttributeKind::VocabProperty, value(json!(" https://example.org/level/high "))),
            Ok(Some(AttributeContent::Vocab(IriBuf::new("https://example.org/level/high".to_owned()).unwrap())))
        );
    }

    #[test]
    fn text_that_is_no_iri_is_refused_quoting_the_text() {
        assert_eq!(
            type_for_kind(NgsiLdAttributeKind::VocabProperty, value(json!("not an iri"))),
            Err(unusable(RequiredValue::Iri, "not an iri"))
        );
    }

    #[test]
    fn a_number_is_no_iri_and_is_refused_quoting_its_rendering() {
        assert_eq!(
            type_for_kind(NgsiLdAttributeKind::VocabProperty, value(json!(20))),
            Err(unusable(RequiredValue::Iri, "20"))
        );
    }

    #[test]
    fn a_blank_vocab_or_language_value_is_absent_rather_than_refused() {
        for kind in [NgsiLdAttributeKind::VocabProperty, NgsiLdAttributeKind::LanguageProperty] {
            assert_eq!(type_for_kind(kind, value(json!(""))), Ok(None));
            assert_eq!(type_for_kind(kind, value(json!("   "))), Ok(None));
        }
    }

    #[test]
    fn an_object_of_tagged_text_becomes_a_language_map_keeping_only_its_usable_entries() {
        assert_eq!(
            type_for_kind(
                NgsiLdAttributeKind::LanguageProperty,
                value(json!({"en": "hello", "pt-BR": "olá", "not a tag!": "x", "fr": 7}))
            ),
            Ok(Some(AttributeContent::Language(language_map(&[("en", "hello"), ("pt-BR", "olá")]))))
        );
    }

    #[test]
    fn plain_text_is_no_language_map_and_is_refused_quoting_the_text() {
        assert_eq!(
            type_for_kind(NgsiLdAttributeKind::LanguageProperty, value(json!("plain text"))),
            Err(unusable(RequiredValue::LanguageMap, "plain text"))
        );
    }

    #[test]
    fn an_object_with_no_tagged_text_is_refused_quoting_its_rendering() {
        assert_eq!(
            type_for_kind(NgsiLdAttributeKind::LanguageProperty, value(json!({"not a tag!": "hello", "en": 7}))),
            Err(unusable(RequiredValue::LanguageMap, r#"{"not a tag!":"hello","en":7}"#))
        );
    }

    #[test]
    fn a_json_property_value_becomes_raw_json_or_is_refused() {
        assert_eq!(
            type_for_kind(NgsiLdAttributeKind::JsonProperty, value(json!({"k": 1}))),
            Ok(Some(AttributeContent::Json(JsonPayload::try_from(json!({"k": 1})).unwrap())))
        );
        assert_eq!(
            type_for_kind(NgsiLdAttributeKind::JsonProperty, value(json!("text"))),
            Err(AttributeRefusal::JsonPayload(JsonPayloadError::NotAnObject { found: JsonShape::String }))
        );
    }

    #[test]
    fn a_kind_whose_value_is_a_value_passes_its_content_through() {
        for kind in [
            NgsiLdAttributeKind::Property,
            NgsiLdAttributeKind::GeoProperty,
            NgsiLdAttributeKind::ListProperty,
            NgsiLdAttributeKind::Relationship,
            NgsiLdAttributeKind::ListRelationship,
        ] {
            assert_eq!(type_for_kind(kind, value(json!("not an iri"))), Ok(Some(value(json!("not an iri")))));
        }
    }

    #[test]
    fn content_already_typed_passes_through() {
        let map = AttributeContent::Language(language_map(&[("en", "hello")]));

        assert_eq!(type_for_kind(NgsiLdAttributeKind::LanguageProperty, map.clone()), Ok(Some(map)));
    }
}
