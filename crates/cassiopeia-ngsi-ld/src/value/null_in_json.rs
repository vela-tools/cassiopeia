use thiserror::Error;

/// JSON that was expected to be a valid NGSI-LD Value as it stands held a null.
///
/// `null` cannot be used as a value anywhere in NGSI-LD (ETSI GS CIM 009 v1.9.1 clause 4.5.0), at
/// the top level or nested inside an object or array.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
#[error("the JSON holds a null, and null cannot be used as a value anywhere in NGSI-LD")]
pub struct NullInJson;

#[cfg(test)]
mod tests {
    use crate::value::null_in_json::NullInJson;

    #[test]
    fn the_message_names_null_as_the_problem() {
        assert!(NullInJson.to_string().contains("null"));
    }
}
