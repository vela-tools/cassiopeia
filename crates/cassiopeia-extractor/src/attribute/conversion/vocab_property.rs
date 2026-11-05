use crate::{
    attribute::{
        conversion::{readers::SourceParts, value_reading},
        refusal::AttributeRefusal,
    },
    required_value::RequiredValue,
};
use cassiopeia_ngsi_ld::value::omitted_nulls::OmittedNulls;
use iri_rs::IriBuf;

/// Converts the parts into the IRI a `VocabProperty`'s `vocab` member names (ETSI GS CIM 009 v1.9.1
/// clause 4.5.20), read from their text without the whitespace around it.
///
/// A blank value is one the record does not carry, so it yields no IRI rather than a refusal.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting text that is no IRI, and any other value as
/// its rendering, as not [`RequiredValue::Iri`].
pub(crate) fn apply(parts: SourceParts, omitted: &mut OmittedNulls) -> Result<Option<IriBuf>, AttributeRefusal> {
    let Some(value) = value_reading::read_text(parts, omitted) else {
        return Ok(None);
    };
    let Some(text) = value.reported_text() else {
        return Ok(None);
    };

    IriBuf::new(text.to_string())
        .map(Some)
        .map_err(|_| AttributeRefusal::unusable(RequiredValue::Iri, &text))
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::fixtures::{apply, default_of},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_ir::attribute_content::AttributeContent;
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use smallvec::smallvec;

    fn vocab(part: JsonValue) -> Result<Option<AttributeContent>, AttributeRefusal> {
        apply(smallvec![part], default_of(NgsiLdAttributeKind::VocabProperty), None)
    }

    #[test]
    fn an_iri_becomes_vocab_content_without_the_whitespace_around_it() {
        assert_eq!(
            vocab(json!(" https://example.org/level/high ")),
            Ok(Some(AttributeContent::Vocab(IriBuf::new("https://example.org/level/high".to_owned()).unwrap())))
        );
    }

    #[test]
    fn text_that_is_no_iri_is_refused_quoting_the_text() {
        assert_eq!(vocab(json!("not an iri")), Err(AttributeRefusal::unusable(RequiredValue::Iri, "not an iri")));
    }

    #[test]
    fn a_number_is_no_iri_and_is_refused_quoting_its_rendering() {
        assert_eq!(vocab(json!(20)), Err(AttributeRefusal::unusable(RequiredValue::Iri, "20")));
    }

    #[test]
    fn an_absent_vocab_source_is_absent_rather_than_refused() {
        for part in [json!(""), json!("   "), JsonValue::Null] {
            assert_eq!(vocab(part), Ok(None));
        }
    }
}
