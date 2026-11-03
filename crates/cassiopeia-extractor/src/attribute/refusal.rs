use crate::required_value::RequiredValue;
use cassiopeia_geometry::error::GeometryError;
use cassiopeia_ngsi_ld::value::json_payload_error::JsonPayloadError;
use derive_more::From;

/// Why an attribute could not take the value its source carried.
///
/// No refusal fails the record: the attribute drops, the entity is still emitted, and the run names
/// what it lost. They share one type because they arrive from one place (applying an attribute's
/// declared transformation) and are told apart again where each is bound to the sink that gathers
/// it.
#[derive(Clone, Debug, Eq, From, PartialEq)]
pub(crate) enum AttributeRefusal {
    /// The source carried a geometry the declared type cannot hold, or one this mapping did not
    /// authorise converting.
    Geometry(GeometryError),
    /// A `JsonProperty`'s source was not "a raw JSON object (or array of objects)" (ETSI GS CIM 009
    /// v1.9.1 clause 4.5.24.2).
    JsonPayload(JsonPayloadError),
    /// The source carried text that reads as no supported spelling of a date-time.
    #[from(ignore)]
    UnreadableTimestamp {
        /// The text that would not read, quoted back so a mapping author can see its shape.
        text: Box<str>,
    },
    /// The source carried a value the declared kind cannot hold: text that is no IRI for a
    /// `VocabProperty`, or anything but tagged text for a `LanguageProperty`.
    #[from(ignore)]
    UnusableValue {
        /// What the kind needed the value to be.
        required: RequiredValue,
        /// The value that could not be used, quoted back so a mapping author can see its shape.
        text: Box<str>,
    },
}

#[cfg(test)]
mod tests {
    use crate::attribute::refusal::AttributeRefusal;
    use cassiopeia_geometry::error::GeometryError;
    use cassiopeia_ngsi_ld::value::json_payload_error::{JsonPayloadError, JsonShape};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_geometry_error_converts_into_a_refusal() {
        assert_eq!(
            AttributeRefusal::from(GeometryError::GeometryCollection),
            AttributeRefusal::Geometry(GeometryError::GeometryCollection)
        );
    }

    #[test]
    fn a_json_payload_error_converts_into_a_refusal() {
        let error = JsonPayloadError::NotAnObject { found: JsonShape::Number };

        assert_eq!(AttributeRefusal::from(error), AttributeRefusal::JsonPayload(error));
    }
}
