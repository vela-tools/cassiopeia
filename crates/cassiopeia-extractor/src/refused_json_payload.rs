use cassiopeia_ngsi_ld::{entity::name::NameBuf, value::json_payload_error::JsonPayloadError};
use derive_more::Display;

/// One `JsonProperty` dropped because its source was not "a raw JSON object (or array of objects)"
/// (ETSI GS CIM 009 v1.9.1 clause 4.5.24.2).
///
/// The attribute and the refusal together are the diagnostic and its identity, so a run reports the
/// problem once and counts how often it happened.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
#[display("Attribute `{attribute}` dropped: {refusal}")]
pub struct RefusedJsonPayload {
    /// The attribute that lost its value.
    pub attribute: NameBuf,
    /// Why its source could not be the `json` member.
    pub refusal: JsonPayloadError,
}

#[cfg(test)]
mod tests {
    use crate::refused_json_payload::RefusedJsonPayload;
    use cassiopeia_ngsi_ld::{
        entity::name::NameBuf,
        value::json_payload_error::{JsonPayloadError, JsonShape},
    };

    #[test]
    fn the_message_names_the_attribute_and_the_reason() {
        let refused = RefusedJsonPayload {
            attribute: NameBuf::new("payload").expect("valid name"),
            refusal: JsonPayloadError::NotAnObject { found: JsonShape::Number },
        };
        let message = refused.to_string();

        assert!(message.contains("`payload`"), "{message}");
        assert!(message.contains("a number"), "{message}");
    }
}
