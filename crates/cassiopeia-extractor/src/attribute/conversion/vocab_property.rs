use crate::{
    attribute::{
        conversion::{attribute_conversion::AttributeConversion, identifier::Identifier, read_context::ReadContext, readers::SourceParts, value_reading},
        refusal::AttributeRefusal,
    },
    required_value::RequiredValue,
};
use cassiopeia_mapping::conversion::value_reading::IdentifierReading;
use cassiopeia_ngsi_ld::value::omitted_nulls::OmittedNulls;
use iri_rs::IriBuf;
use serde_json::Value as JsonValue;

/// The IRI a `VocabProperty`'s `vocab` member names (ETSI GS CIM 009 v1.9.1 clause 4.5.20), read
/// from the parts as a `Property` reads the identifier the reading names, and written in its normal
/// form (see [`value_reading::read_identifier`]).
///
/// A null or blank value is one the record does not carry, so it yields no IRI rather than a
/// refusal; text that is not the identifier is refused quoting it, and any other value as its
/// rendering. A URN is an absolute IRI (RFC 8141 clause 2 and RFC 3987 clause 2.2), so admitting
/// writes it as the IRI the type holds.
impl AttributeConversion for IdentifierReading {
    type Content = IriBuf;
    type Reading = Identifier;

    fn read(self, parts: SourceParts, ctx: &mut ReadContext<'_>) -> Result<Option<Identifier>, AttributeRefusal> {
        value_reading::read_identifier(parts, self, ctx.omitted)
    }

    /// # Errors
    /// Returns [`AttributeRefusal::UnusableValue`] quoting a URN that `iri-rs`, which parses IRIs
    /// apart from the URN parser, does not read as an IRI, as not [`RequiredValue::Iri`].
    fn admit(self, reading: Identifier, _omitted: &mut OmittedNulls) -> Result<Option<IriBuf>, AttributeRefusal> {
        match reading {
            Identifier::Iri(iri) => Ok(Some(iri)),
            Identifier::Urn(urn) => IriBuf::new(urn.to_string())
                .map(Some)
                .map_err(|_| AttributeRefusal::unusable(RequiredValue::Iri, urn.as_str())),
        }
    }

    fn into_json(reading: Identifier) -> JsonValue {
        JsonValue::from(reading)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::fixtures::{apply, declared, default_of, member},
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

    #[test]
    fn the_member_read_of_a_vocab_property_is_its_normalized_iri_text() {
        assert_eq!(
            member(
                smallvec![json!("HTTPS://Example.org/level/high")],
                default_of(NgsiLdAttributeKind::VocabProperty)
            )
            .0,
            Ok(Some(json!("https://example.org/level/high")))
        );
    }
}
