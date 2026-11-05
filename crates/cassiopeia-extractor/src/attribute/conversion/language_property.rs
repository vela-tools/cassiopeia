use crate::{
    attribute::{
        conversion::{
            attribute_conversion::AttributeConversion,
            read_context::ReadContext,
            readers::{self, SourceParts},
            value_reading,
        },
        refusal::AttributeRefusal,
    },
    required_value::RequiredValue,
};
use cassiopeia_ir::attribute_content::AttributeContent;
use cassiopeia_ngsi_ld::{
    entity::attribute::language_property::LanguageMap,
    value::{json_payload::JsonObject, omitted_nulls::OmittedNulls, types::Value},
};
use langtag::LangTagBuf;
use serde_json::Value as JsonValue;

/// The conversion of a `LanguageProperty` read from one source into its `languageMap` (ETSI GS CIM
/// 009 v1.9.1 clause 4.5.18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LanguageMapConversion;

/// The conversion of one `languageMap` entry into the localized string it holds (ETSI GS CIM 009
/// v1.9.1 clause 4.5.18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LanguageTextConversion;

/// Reads one object of text keyed by language tag, or its JSON text, as the `languageMap`.
///
/// Only an entry keyed by a well-formed BCP-47 tag and holding text enters the map; any other entry,
/// a null one included, is no localized string and is left out. Blank text and a null value are
/// values the record does not carry and yield no map. A value that is no object, or an object with
/// no entry that can enter the map, is refused as not [`RequiredValue::LanguageMap`]: an empty
/// `languageMap` carries no value at all. Reading builds the map the type holds, so admitting keeps
/// it as it is.
impl AttributeConversion for LanguageMapConversion {
    type Content = LanguageMap;
    type Reading = LanguageMap;

    fn read(self, parts: SourceParts, _ctx: &mut ReadContext<'_>) -> Result<Option<LanguageMap>, AttributeRefusal> {
        match readers::merge_text(parts) {
            Some(JsonValue::Object(entries)) => language_map(entries).map(Some),
            Some(JsonValue::String(text)) => match readers::read_json(&text, RequiredValue::LanguageMap)? {
                Some(JsonValue::Object(entries)) => language_map(entries).map(Some),
                Some(JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) | JsonValue::Array(_)) => {
                    Err(AttributeRefusal::unusable(RequiredValue::LanguageMap, &text))
                }
                None => Ok(None),
            },
            Some(other @ (JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Array(_))) => {
                Err(AttributeRefusal::unusable(RequiredValue::LanguageMap, &other.to_string()))
            }
            Some(JsonValue::Null) | None => Ok(None),
        }
    }

    fn admit(self, reading: LanguageMap, _omitted: &mut OmittedNulls) -> Result<Option<LanguageMap>, AttributeRefusal> {
        Ok(Some(reading))
    }

    fn into_json(reading: LanguageMap) -> JsonValue {
        JsonValue::from(AttributeContent::Language(reading))
    }
}

/// Reads an entry's value as text: text as it is, and any other value as its compact JSON text (see
/// [`value_reading::read_text`]). Reading never refuses an entry, and admitting keeps the text as it
/// is.
impl AttributeConversion for LanguageTextConversion {
    type Content = Value;
    type Reading = Value;

    fn read(self, parts: SourceParts, ctx: &mut ReadContext<'_>) -> Result<Option<Value>, AttributeRefusal> {
        Ok(value_reading::read_text(parts, ctx.omitted))
    }

    fn admit(self, reading: Value, _omitted: &mut OmittedNulls) -> Result<Option<Value>, AttributeRefusal> {
        Ok(Some(reading))
    }

    fn into_json(reading: Value) -> JsonValue {
        JsonValue::from(reading)
    }
}

/// The entries of an object that can enter a `languageMap`.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting the object as its compact JSON text when no
/// entry can.
fn language_map(entries: JsonObject) -> Result<LanguageMap, AttributeRefusal> {
    let map = entries
        .iter()
        .filter_map(|(language, text)| Some((LangTagBuf::new(language.clone()).ok()?, text.as_str()?.to_owned())))
        .collect::<LanguageMap>();

    if map.is_empty() {
        Err(AttributeRefusal::unusable(RequiredValue::LanguageMap, &JsonValue::Object(entries).to_string()))
    } else {
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::fixtures::{apply, converted, default_of},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_ir::attribute_content::AttributeContent;
    use cassiopeia_mapping::conversion::{Conversion, declaration_shape::DeclarationShape};
    use cassiopeia_ngsi_ld::{
        entity::attribute::{NgsiLdAttributeKind, language_property::LanguageMap},
        value::types::Value,
    };
    use langtag::LangTagBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use smallvec::smallvec;

    fn language(part: JsonValue) -> Result<Option<AttributeContent>, AttributeRefusal> {
        apply(smallvec![part], default_of(NgsiLdAttributeKind::LanguageProperty), None)
    }

    fn language_map(entries: &[(&str, &str)]) -> LanguageMap {
        entries
            .iter()
            .map(|(tag, text)| (LangTagBuf::new((*tag).to_owned()).unwrap(), (*text).to_owned()))
            .collect()
    }

    #[test]
    fn an_object_of_tagged_text_becomes_a_language_map_keeping_only_its_usable_entries() {
        assert_eq!(
            language(json!({"en": "hello", "pt-BR": "olá", "not a tag!": "x", "fr": 7, "de": null})),
            Ok(Some(AttributeContent::Language(language_map(&[("en", "hello"), ("pt-BR", "olá")]))))
        );
    }

    #[test]
    fn the_json_text_of_a_map_becomes_the_language_map() {
        assert_eq!(
            language(json!(r#"{"en":"Hello"}"#)),
            Ok(Some(AttributeContent::Language(language_map(&[("en", "Hello")]))))
        );
    }

    #[test]
    fn plain_text_is_no_language_map_and_is_refused_quoting_the_text() {
        assert_eq!(
            language(json!("plain text")),
            Err(AttributeRefusal::unusable(RequiredValue::LanguageMap, "plain text"))
        );
    }

    #[test]
    fn an_object_with_no_tagged_text_is_refused_quoting_its_rendering() {
        assert_eq!(
            language(json!({"not a tag!": "hello", "en": 7})),
            Err(AttributeRefusal::unusable(RequiredValue::LanguageMap, r#"{"not a tag!":"hello","en":7}"#))
        );
    }

    #[test]
    fn a_blank_language_source_is_absent_rather_than_refused() {
        for part in [json!(""), json!("   "), JsonValue::Null] {
            assert_eq!(language(part), Ok(None));
        }
    }

    #[test]
    fn a_language_map_entry_reads_its_value_as_text() {
        let entry = Conversion::resolve(NgsiLdAttributeKind::LanguageProperty, None, DeclarationShape::LanguageEntry).unwrap();

        assert_eq!(converted(smallvec![json!(12)], entry, None), Some(Value::String("12".into())));
    }
}
