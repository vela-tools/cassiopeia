use crate::attribute::conversion::attribute_conversion::ConversionOutput;
use cassiopeia_ir::{attribute_content::AttributeContent, resolved_attribute::ResolvedAttribute};
use serde_json::Value as JsonValue;

/// What resolving one declaration produces, decided by where the declaration sits: a top-level
/// attribute, a sub-attribute, or a member of an object nested `mappings` assemble.
///
/// Every place resolves a declaration through the same walk; only what its value converts into, and
/// how that value stands where the declaration sits, differ.
pub(crate) trait DeclarationOutput: Sized {
    /// What the declaration's value converts into.
    type Content: ConversionOutput;

    /// What a declaration resolving to `content` produces.
    fn single(content: Self::Content) -> Self;
}

/// A top-level attribute: the one value it resolved to.
impl DeclarationOutput for ResolvedAttribute {
    type Content = AttributeContent;

    fn single(content: AttributeContent) -> ResolvedAttribute {
        ResolvedAttribute::Single(content)
    }
}

/// A sub-attribute: the one content it resolved to.
impl DeclarationOutput for AttributeContent {
    type Content = AttributeContent;

    fn single(content: AttributeContent) -> AttributeContent {
        content
    }
}

/// A member of an object nested `mappings` assemble: the raw JSON it contributes.
impl DeclarationOutput for JsonValue {
    type Content = JsonValue;

    fn single(content: JsonValue) -> JsonValue {
        content
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::declaration_output::DeclarationOutput;
    use cassiopeia_ir::{attribute_content::AttributeContent, resolved_attribute::ResolvedAttribute};
    use cassiopeia_ngsi_ld::value::types::Value;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    #[test]
    fn a_top_level_attribute_of_one_value_is_single() {
        let content = AttributeContent::Value(Value::Boolean(true));

        assert_eq!(
            <ResolvedAttribute as DeclarationOutput>::single(content.clone()),
            ResolvedAttribute::Single(content)
        );
    }

    #[test]
    fn a_sub_attribute_is_the_content_it_resolved_to() {
        let content = AttributeContent::Value(Value::Boolean(true));

        assert_eq!(<AttributeContent as DeclarationOutput>::single(content.clone()), content);
    }

    #[test]
    fn a_member_is_the_raw_json_it_resolved_to_keeping_its_nulls() {
        assert_eq!(<JsonValue as DeclarationOutput>::single(json!({"a": null})), json!({"a": null}));
    }
}
