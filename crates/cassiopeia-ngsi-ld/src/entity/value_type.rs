use crate::{
    entity::error::{NgsiLdError, Result},
    value::types::TemporalValueType,
};
use compact_str::CompactString;
use derive_more::Display;
use iri_rs::IriRef;
use lazy_regex::regex_is_match;
use serde::{Deserialize, Serialize};

/// The datatype of an attribute's value, as its `valueType` member names it (ETSI GS CIM 009 v1.9.1
/// clause 4.5.2).
///
/// `valueType` is a non-reified alternative to a native JSON-LD `@type`: the core `@context` defines
/// it as `ngsi-ld:hasValueType` with `@type: @vocab`, so its text is type coerced into a datatype IRI
/// when the attribute is expanded. The text is held exactly as written, a term (`PostalAddress`), a
/// compact IRI (`xsd:decimal`) or an absolute IRI, because the `@context` sent with the entity
/// expands it the same way at the broker. Construction refuses text that could never expand to an
/// IRI: text that is no IRI reference at all (RFC 3987), the empty string, and a JSON-LD keyword-like
/// form such as `@id`, which IRI expansion turns into nothing.
///
/// The text is a [`CompactString`] for the reason [`NameBuf`](crate::entity::name::NameBuf) is one:
/// the same few datatypes are copied into every entity a mapping produces, and most of them are
/// short enough to live inline.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(try_from = "CompactString", into = "CompactString")]
pub struct ValueType(IriRef<CompactString>);

impl ValueType {
    /// Builds a value type from the text a mapping wrote.
    ///
    /// # Errors
    /// Returns [`NgsiLdError::InvalidValueType`] when `text` is empty, is keyword-like, or is no IRI
    /// reference.
    pub fn new(text: impl Into<CompactString>) -> Result<ValueType> {
        let text = text.into();
        // JSON-LD 1.1 IRI expansion maps any keyword-like value (`@` followed by letters) to null.
        if text.is_empty() || regex_is_match!(r"^@[a-zA-Z]+$", &text) {
            return Err(NgsiLdError::InvalidValueType {
                rejected: text.as_str().into(),
            });
        }

        IriRef::parse(text).map(ValueType).map_err(|rejected| NgsiLdError::InvalidValueType {
            rejected: rejected.0.as_str().into(),
        })
    }

    /// The `langString` term, the only value type a `LanguageProperty` may carry (ETSI GS CIM 009
    /// v1.9.1 table 5.2.32-1).
    #[must_use]
    pub fn lang_string() -> ValueType {
        // `langString` is a plain term, and so a valid relative IRI reference.
        ValueType(IriRef::parse_unchecked(CompactString::const_new("langString")))
    }

    /// The value type as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<TemporalValueType> for ValueType {
    fn from(temporal: TemporalValueType) -> ValueType {
        // The three terms clause 4.6.3 names are plain terms, and so valid relative IRI references.
        ValueType(IriRef::parse_unchecked(CompactString::const_new(temporal.into())))
    }
}

impl TryFrom<CompactString> for ValueType {
    type Error = NgsiLdError;

    fn try_from(text: CompactString) -> Result<ValueType> {
        ValueType::new(text)
    }
}

impl From<ValueType> for CompactString {
    fn from(value_type: ValueType) -> CompactString {
        CompactString::from(value_type.as_str())
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity::value_type::ValueType, value::types::TemporalValueType};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_term_a_compact_iri_and_an_absolute_iri_are_kept_as_written() {
        for text in ["PostalAddress", "xsd:decimal", "ngsi-ld:DateTime", "http://www.w3.org/2001/XMLSchema#duration"] {
            assert_eq!(ValueType::new(text).unwrap().as_str(), text);
        }
    }

    #[test]
    fn empty_text_is_refused() {
        assert!(ValueType::new("").is_err());
    }

    #[test]
    fn a_keyword_like_value_that_expands_to_nothing_is_refused() {
        assert!(ValueType::new("@id").is_err());
        assert!(ValueType::new("@json").is_err());
    }

    #[test]
    fn text_that_is_no_iri_reference_is_refused_naming_the_text() {
        let error = ValueType::new("Postal Address").unwrap_err();

        assert!(error.to_string().contains("'Postal Address'"));
    }

    #[test]
    fn a_value_type_round_trips_through_json_as_a_plain_string() {
        let value_type: ValueType = serde_json::from_str(r#""xsd:anyURI""#).unwrap();

        assert_eq!(value_type, ValueType::new("xsd:anyURI").unwrap());
        assert_eq!(serde_json::to_string(&value_type).unwrap(), r#""xsd:anyURI""#);
        assert!(serde_json::from_str::<ValueType>(r#""@type""#).is_err());
    }

    #[test]
    fn a_time_data_type_becomes_the_term_clause_4_6_3_names_it_by() {
        assert_eq!(ValueType::from(TemporalValueType::DateTime).as_str(), "DateTime");
        assert_eq!(ValueType::from(TemporalValueType::Date).as_str(), "Date");
        assert_eq!(ValueType::from(TemporalValueType::Time).as_str(), "Time");
    }

    #[test]
    fn the_language_property_value_type_is_the_lang_string_term() {
        assert_eq!(ValueType::lang_string().as_str(), "langString");
    }
}
