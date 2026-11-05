use crate::attribute::conversion::attribute_conversion::ConversionOutput;
use cassiopeia_ir::{attribute_content::AttributeContent, resolved_attribute::ResolvedAttribute};
use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};
use serde_json::Value as JsonValue;

/// What resolving one declaration produces, decided by where the declaration sits: a top-level
/// attribute, a sub-attribute, or a member of an object nested `mappings` assemble.
///
/// Every place resolves a declaration through the same walk; only what one value converts into, and
/// how the values of a declaration declaring instances combine, differ.
pub(crate) trait DeclarationOutput: Sized {
    /// What one value of the declaration converts into.
    type Content: ConversionOutput;

    /// What a declaration resolving to one value produces.
    fn single(content: Self::Content) -> Self;

    /// What a declaration declaring instances produces, from what each instance resolved to, in
    /// declared order, at least one of them present, counting in `omitted` each null left out.
    fn instances(contents: Vec<Option<Self::Content>>, omitted: &mut OmittedNulls) -> Self;
}

/// A top-level attribute: one entry per declared instance.
///
/// An instance that resolved to nothing stays `None`, so entry `i` stays aligned with the metadata
/// of instance `i` (ETSI GS CIM 009 v1.9.1 clause 4.5.5).
impl DeclarationOutput for ResolvedAttribute {
    type Content = AttributeContent;

    fn single(content: AttributeContent) -> ResolvedAttribute {
        ResolvedAttribute::Single(content)
    }

    fn instances(contents: Vec<Option<AttributeContent>>, _omitted: &mut OmittedNulls) -> ResolvedAttribute {
        ResolvedAttribute::Instances(contents)
    }
}

/// A sub-attribute: one content, which for a declaration declaring instances is a value listing
/// every instance that resolved.
///
/// Only a Value can stand as an item of that list. Raw JSON becomes a Value without the nulls it
/// holds, which are counted: they were admissible in a `JsonProperty`'s `json` member, but not
/// inside a Property's value (ETSI GS CIM 009 v1.9.1 clause 4.5.0). A vocabulary IRI becomes its
/// text, and a `languageMap` the object of text it is.
impl DeclarationOutput for AttributeContent {
    type Content = AttributeContent;

    fn single(content: AttributeContent) -> AttributeContent {
        content
    }

    fn instances(contents: Vec<Option<AttributeContent>>, omitted: &mut OmittedNulls) -> AttributeContent {
        AttributeContent::Value(Value::Array(
            contents
                .into_iter()
                .flatten()
                .filter_map(|content| match content {
                    AttributeContent::Value(value) => Some(value),
                    other @ (AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_)) => {
                        Value::from_json(JsonValue::from(other), omitted)
                    }
                })
                .collect(),
        ))
    }
}

/// A member of an object nested `mappings` assemble: the raw JSON it contributes, which for a
/// member declaring instances is the list of the instances that resolved.
impl DeclarationOutput for JsonValue {
    type Content = JsonValue;

    fn single(content: JsonValue) -> JsonValue {
        content
    }

    fn instances(contents: Vec<Option<JsonValue>>, _omitted: &mut OmittedNulls) -> JsonValue {
        JsonValue::Array(contents.into_iter().flatten().collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::declaration_output::DeclarationOutput;
    use cassiopeia_ir::{attribute_content::AttributeContent, resolved_attribute::ResolvedAttribute};
    use cassiopeia_ngsi_ld::value::{json_payload::JsonPayload, omitted_nulls::OmittedNulls, types::Value};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    #[test]
    fn a_top_level_attribute_keeps_an_entry_per_instance_including_the_ones_that_resolved_to_nothing() {
        let contents = vec![None, Some(AttributeContent::Value(Value::Boolean(true)))];
        let mut omitted = OmittedNulls::default();

        assert_eq!(
            <ResolvedAttribute as DeclarationOutput>::instances(contents.clone(), &mut omitted),
            ResolvedAttribute::Instances(contents)
        );
        assert!(omitted.is_empty());
    }

    #[test]
    fn a_top_level_attribute_of_one_value_is_single() {
        let content = AttributeContent::Value(Value::Boolean(true));

        assert_eq!(
            <ResolvedAttribute as DeclarationOutput>::single(content.clone()),
            ResolvedAttribute::Single(content)
        );
    }

    #[test]
    fn a_sub_attribute_lists_the_instances_that_resolved_as_values_counting_the_nulls_it_leaves_out() {
        let contents = vec![
            Some(AttributeContent::Value(Value::Boolean(true))),
            None,
            Some(AttributeContent::Json(JsonPayload::try_from(json!({"a": 1, "b": null})).unwrap())),
            Some(AttributeContent::Vocab(IriBuf::new("https://example.org/x".to_owned()).unwrap())),
        ];
        let mut omitted = OmittedNulls::default();

        assert_eq!(
            <AttributeContent as DeclarationOutput>::instances(contents, &mut omitted),
            AttributeContent::Value(Value::try_from(json!([true, {"a": 1}, "https://example.org/x"])).unwrap())
        );
        assert_eq!(omitted.object_members, 1);
    }

    #[test]
    fn a_member_lists_the_raw_json_of_the_instances_that_resolved_keeping_their_nulls() {
        let mut omitted = OmittedNulls::default();

        assert_eq!(
            <JsonValue as DeclarationOutput>::instances(vec![Some(json!({"a": null})), None, Some(json!(2))], &mut omitted),
            json!([{"a": null}, 2])
        );
        assert!(omitted.is_empty());
    }
}
