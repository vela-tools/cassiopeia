use strum::Display;
use thiserror::Error;

/// The shape of a JSON value that cannot be a `JsonProperty`'s `json` member, as its error names it.
#[derive(Debug, Clone, Copy, Display, PartialEq, Eq, Hash)]
pub enum JsonShape {
    /// JSON `null`.
    #[strum(to_string = "null")]
    Null,
    /// A JSON boolean.
    #[strum(to_string = "a boolean")]
    Boolean,
    /// A JSON number.
    #[strum(to_string = "a number")]
    Number,
    /// A JSON string.
    #[strum(to_string = "a string")]
    String,
    /// A JSON array.
    #[strum(to_string = "an array")]
    Array,
}

/// Why a JSON value cannot be a `JsonProperty`'s `json` member.
///
/// The member is "a raw JSON object (or array of objects)" (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.24.2), so anything else is refused rather than written.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq, Hash)]
pub enum JsonPayloadError {
    /// The value is neither an object nor an array.
    #[error("a JsonProperty's `json` is a JSON object or an array of objects, not {found}")]
    NotAnObject {
        /// What the value is instead.
        found: JsonShape,
    },
    /// The value is an array holding something other than an object.
    #[error("a JsonProperty's `json` array holds only objects, but this one holds {found}")]
    ItemNotAnObject {
        /// What the offending item is instead.
        found: JsonShape,
    },
}

#[cfg(test)]
mod tests {
    use crate::value::json_payload_error::{JsonPayloadError, JsonShape};

    #[test]
    fn the_message_names_the_shape_found() {
        let message = JsonPayloadError::NotAnObject { found: JsonShape::Number }.to_string();

        assert!(message.contains("a number"), "{message}");
    }

    #[test]
    fn the_message_for_an_array_names_the_offending_item_s_shape() {
        let message = JsonPayloadError::ItemNotAnObject { found: JsonShape::String }.to_string();

        assert!(message.contains("a string"), "{message}");
    }
}
