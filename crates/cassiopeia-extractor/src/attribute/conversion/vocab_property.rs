use crate::attribute::{
    conversion::{identifier, readers::SourceParts, value_reading},
    refusal::AttributeRefusal,
};
use cassiopeia_mapping::conversion::value_reading::IdentifierReading;
use cassiopeia_ngsi_ld::value::omitted_nulls::OmittedNulls;
use iri_rs::IriBuf;

/// Converts the parts into the IRI a `VocabProperty`'s `vocab` member names (ETSI GS CIM 009 v1.9.1
/// clause 4.5.20), read from their text as the identifier `reading` names and written in its normal
/// form (see [`identifier::read_identifier`]).
///
/// A blank value is one the record does not carry, so it yields no IRI rather than a refusal.
///
/// # Errors
/// Returns [`AttributeRefusal::UnusableValue`] quoting text that is not the identifier, and any
/// other value as its rendering.
pub(crate) fn apply(parts: SourceParts, reading: IdentifierReading, omitted: &mut OmittedNulls) -> Result<Option<IriBuf>, AttributeRefusal> {
    value_reading::read_text(parts, omitted).map_or(Ok(None), |value| identifier::read_identifier(&value, reading))
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::fixtures::{apply, declared, default_of},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_ir::attribute_content::AttributeContent;
    use cassiopeia_mapping::transformation::Transformation;
    use cassiopeia_ngsi_ld::entity::attribute::NgsiLdAttributeKind;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use smallvec::smallvec;

    fn vocab(part: JsonValue) -> Result<Option<AttributeContent>, AttributeRefusal> {
        apply(smallvec![part], default_of(NgsiLdAttributeKind::VocabProperty), None)
    }

    fn content(iri: &str) -> AttributeContent {
        AttributeContent::Vocab(IriBuf::new(iri.to_owned()).unwrap())
    }

    #[test]
    fn a_vocab_property_reads_its_iri_by_default() {
        assert_eq!(
            vocab(json!("https://example.org/level/high")),
            Ok(Some(content("https://example.org/level/high")))
        );
    }

    #[test]
    fn a_vocab_property_writes_its_iri_normalized() {
        assert_eq!(
            vocab(json!(" HTTPS://Example.org/level/high ")),
            Ok(Some(content("https://example.org/level/high")))
        );
    }

    #[test]
    fn a_vocab_property_refuses_text_that_is_no_iri() {
        assert_eq!(vocab(json!("not an iri")), Err(AttributeRefusal::unusable(RequiredValue::Iri, "not an iri")));
        assert_eq!(vocab(json!(12)), Err(AttributeRefusal::unusable(RequiredValue::Iri, "12")));
    }

    #[test]
    fn urn_on_a_vocab_property_admits_only_a_urn() {
        let urn = declared(NgsiLdAttributeKind::VocabProperty, Transformation::Urn);

        assert_eq!(
            apply(smallvec![json!("urn:ngsi-ld:vocab:electricMicrobus")], urn, None),
            Ok(Some(content("urn:ngsi-ld:vocab:electricMicrobus")))
        );
        assert_eq!(
            apply(smallvec![json!("https://x.org")], urn, None),
            Err(AttributeRefusal::unusable(RequiredValue::Urn, "https://x.org"))
        );
    }

    #[test]
    fn an_absent_vocab_source_is_absent() {
        for part in [json!(""), json!("   "), JsonValue::Null] {
            assert_eq!(vocab(part), Ok(None));
        }
    }
}
