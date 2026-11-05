use crate::attribute::refusal::AttributeRefusal;
use cassiopeia_mapping::conversion::value_reading::{IdentifierReading, ValueReading};
use cassiopeia_ngsi_ld::value::types::Value;
use compact_str::CompactString;
use iri_rs::IriBuf;
use serde_json::Value as JsonValue;
use urn_rs::Urn;

/// An identifier an identifier reading parsed, in its normal form, kept as the type it was parsed
/// as.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Identifier {
    /// An absolute IRI (RFC 3987 clause 2.2).
    Iri(IriBuf),
    /// A URN (RFC 8141 clause 2).
    Urn(Urn),
}

/// Writes an identifier as the text value a `Property` holds.
// Manual because each variant moves the text it owns into the value, which no derive expresses.
impl From<Identifier> for Value {
    fn from(identifier: Identifier) -> Value {
        Value::String(match identifier {
            Identifier::Iri(iri) => CompactString::from(iri.into_inner()),
            Identifier::Urn(urn) => CompactString::from(urn.as_str()),
        })
    }
}

/// Writes an identifier as its text inside a structured value, such as a member of an object nested
/// `mappings` assemble.
// Manual because each variant moves the text it owns into the JSON, which no derive expresses.
impl From<Identifier> for JsonValue {
    fn from(identifier: Identifier) -> JsonValue {
        JsonValue::String(match identifier {
            Identifier::Iri(iri) => iri.into_inner(),
            Identifier::Urn(urn) => urn.to_string(),
        })
    }
}

/// Reads a value the record carries as the identifier `reading` names, in its normal form, from its
/// text without the whitespace around it.
///
/// An IRI is parsed as an absolute IRI (RFC 3987 clause 2.2) and normalized only in the ways that
/// keep it equivalent: the syntax-based normalization of RFC 3986 clause 6.2.2, which lowercases
/// the scheme and the host, writes percent-encodings in uppercase hex, decodes the encodings of
/// unreserved characters, and removes dot segments, and, for `http` and `https`, the scheme-based
/// normalization of clause 6.2.3, which drops the default port and writes an empty path as `/`. A
/// URN is parsed by RFC 8141 clause 2 and written as `urn-rs` normalizes it, with the `urn:` scheme
/// and the namespace identifier lowercased.
///
/// A blank value is one the record does not carry; the caller reads it as no value before it
/// reaches here.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting the value (see [`Value::quoted`]) as not the
/// identifier the reading requires (see [`AttributeRefusal::unreadable`]).
pub(crate) fn read_identifier(value: &Value, reading: IdentifierReading) -> Result<Identifier, AttributeRefusal> {
    let text = value.quoted();
    let identifier = match reading {
        IdentifierReading::Iri => IriBuf::new(text.to_string()).ok().map(|iri| Identifier::Iri(iri.normalized_for_scheme())),
        IdentifierReading::Urn => text.parse::<Urn>().ok().map(Identifier::Urn),
    };

    identifier.ok_or_else(|| AttributeRefusal::unreadable(ValueReading::Identifier(reading), &text))
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::identifier::{Identifier, read_identifier},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_mapping::conversion::value_reading::IdentifierReading;
    use cassiopeia_ngsi_ld::value::types::Value;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    fn read(json: JsonValue, reading: IdentifierReading) -> Result<Value, AttributeRefusal> {
        read_identifier(&Value::try_from(json).unwrap(), reading).map(Value::from)
    }

    /// The text value `text` is.
    fn text_value(text: &str) -> Value {
        Value::try_from(json!(text)).unwrap()
    }

    #[test]
    fn an_iri_is_written_in_its_normal_form() {
        for (text, normal) in [
            ("  HTTP://Example.org/a ", "http://example.org/a"),
            ("http://User@Example.org/", "http://User@example.org/"),
            ("http://example.org/%7efoo%2f", "http://example.org/~foo%2F"),
            ("http://example.org/a/./b/../c", "http://example.org/a/c"),
            ("http://example.org:80/a", "http://example.org/a"),
            ("https://example.org:443/a", "https://example.org/a"),
            ("http://example.org:8080/a", "http://example.org:8080/a"),
            ("http://example.org", "http://example.org/"),
            ("mailto:A@B.org", "mailto:A@B.org"),
            ("http://example.org/A?B=C#D", "http://example.org/A?B=C#D"),
        ] {
            assert_eq!(read(json!(text), IdentifierReading::Iri), Ok(text_value(normal)), "{text}");
        }
    }

    #[test]
    fn text_that_is_no_absolute_iri_is_refused_quoting_it() {
        for text in ["not an iri", "/a/b"] {
            assert_eq!(
                read(json!(text), IdentifierReading::Iri),
                Err(AttributeRefusal::unusable(RequiredValue::Iri, text))
            );
        }
        assert_eq!(
            read(json!(12), IdentifierReading::Iri),
            Err(AttributeRefusal::unusable(RequiredValue::Iri, "12"))
        );
    }

    #[test]
    fn a_urn_is_written_in_its_normal_form() {
        for (text, normal) in [
            ("URN:NGSI-LD:Building:B-12", "urn:ngsi-ld:Building:B-12"),
            ("urn:ex:%7e", "urn:ex:%7E"),
            (" urn:ngsi-ld:vocab:electricMicrobus ", "urn:ngsi-ld:vocab:electricMicrobus"),
        ] {
            assert_eq!(read(json!(text), IdentifierReading::Urn), Ok(text_value(normal)), "{text}");
        }
    }

    #[test]
    fn text_that_is_no_urn_is_refused_quoting_it() {
        for text in ["B-12", "http://example.org"] {
            assert_eq!(
                read(json!(text), IdentifierReading::Urn),
                Err(AttributeRefusal::unusable(RequiredValue::Urn, text))
            );
        }
    }

    #[test]
    fn an_iri_is_kept_as_an_iri_and_a_urn_as_a_urn() {
        let value = |text: &str| Value::try_from(json!(text)).unwrap();

        assert!(matches!(read_identifier(&value("urn:ex:a"), IdentifierReading::Iri), Ok(Identifier::Iri(_))));
        assert!(matches!(read_identifier(&value("urn:ex:a"), IdentifierReading::Urn), Ok(Identifier::Urn(_))));
    }

    #[test]
    fn an_identifier_is_written_as_its_text_inside_a_structured_value() {
        for (text, reading) in [("URN:Ex:a", IdentifierReading::Urn), ("HTTP://A.org", IdentifierReading::Iri)] {
            let identifier = read_identifier(&Value::try_from(json!(text)).unwrap(), reading).unwrap();

            assert_eq!(JsonValue::from(identifier.clone()), JsonValue::from(Value::from(identifier)), "{text}");
        }
    }
}
