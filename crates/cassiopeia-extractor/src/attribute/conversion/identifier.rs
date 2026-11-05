use crate::attribute::refusal::AttributeRefusal;
use cassiopeia_mapping::conversion::value_reading::{IdentifierReading, ValueReading};
use cassiopeia_ngsi_ld::value::types::Value;
use iri_rs::IriBuf;
use urn_rs::Urn;

/// Reads a value's text, without the whitespace around it, as the identifier `reading` names, in
/// its normal form.
///
/// An IRI is parsed as an absolute IRI (RFC 3987 clause 2.2) and normalized only in the ways that
/// keep it equivalent: the syntax-based normalization of RFC 3986 clause 6.2.2, which lowercases
/// the scheme and the host, writes percent-encodings in uppercase hex, decodes the encodings of
/// unreserved characters, and removes dot segments, and, for `http` and `https`, the scheme-based
/// normalization of clause 6.2.3, which drops the default port and writes an empty path as `/`. A
/// URN is parsed by RFC 8141 clause 2 and written as `urn-rs` normalizes it, with the `urn:` scheme
/// and the namespace identifier lowercased; every URN is an absolute IRI, so it becomes one.
///
/// A blank value is one the record does not carry, so it yields no identifier rather than a
/// refusal.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting text that is not the identifier, and any
/// other value as its rendering, as not the identifier the reading requires (see
/// [`AttributeRefusal::unreadable`]).
pub(crate) fn read_identifier(value: &Value, reading: IdentifierReading) -> Result<Option<IriBuf>, AttributeRefusal> {
    let Some(text) = value.reported_text() else {
        return Ok(None);
    };

    let identifier = match reading {
        IdentifierReading::Iri => IriBuf::new(text.to_string()).ok().map(|iri| iri.normalized_for_scheme()),
        IdentifierReading::Urn => text.parse::<Urn>().ok().and_then(|urn| IriBuf::new(urn.as_str().to_owned()).ok()),
    };

    identifier
        .map(Some)
        .ok_or_else(|| AttributeRefusal::unreadable(ValueReading::Identifier(reading), &text))
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{conversion::identifier::read_identifier, refusal::AttributeRefusal},
        required_value::RequiredValue,
    };
    use cassiopeia_mapping::conversion::value_reading::IdentifierReading;
    use cassiopeia_ngsi_ld::value::types::Value;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    fn read(json: JsonValue, reading: IdentifierReading) -> Result<Option<String>, AttributeRefusal> {
        read_identifier(&Value::try_from(json).unwrap(), reading).map(|iri| iri.map(|iri| iri.as_str().to_owned()))
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
            assert_eq!(read(json!(text), IdentifierReading::Iri), Ok(Some(normal.to_owned())), "{text}");
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
            assert_eq!(read(json!(text), IdentifierReading::Urn), Ok(Some(normal.to_owned())), "{text}");
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
    fn a_blank_identifier_is_absent_rather_than_refused() {
        for reading in [IdentifierReading::Iri, IdentifierReading::Urn] {
            assert_eq!(read(json!(""), reading), Ok(None));
            assert_eq!(read(json!("   "), reading), Ok(None));
        }
    }
}
