use crate::required_value::RequiredValue;
use cassiopeia_geometry::error::GeometryError;
use cassiopeia_mapping::conversion::value_reading::{IdentifierReading, ValueReading};
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
    /// The source carried a value the declaration cannot use: a value the `boolean`, `integer`, or
    /// `float` transformation finds no such scalar in, text that is no IRI for a `VocabProperty`,
    /// anything but tagged text for a `LanguageProperty`, text the `json` transformation cannot
    /// read, a value the `object` transformation finds no object in, an object assembled from
    /// nested `mappings` that a geometry transformation finds no geometry in, or an object or array
    /// holding nothing but nulls for a `Property` or a `ListProperty`.
    #[from(ignore)]
    UnusableValue {
        /// What the kind or transformation needed the value to be.
        required: RequiredValue,
        /// The value that could not be used, quoted back so a mapping author can see its shape.
        text: Box<str>,
    },
}

impl AttributeRefusal {
    /// The refusal of a value that is not the `required` one, quoting the text it was met with.
    pub(crate) fn unusable(required: RequiredValue, text: &str) -> AttributeRefusal {
        AttributeRefusal::UnusableValue {
            required,
            text: Box::from(text),
        }
    }

    /// The refusal of a value that is there but that `reading` could not read, quoting the text it
    /// was met with, under what the reading requires.
    ///
    /// A temporal reading names an unreadable timestamp. The text reading reads every value, so a
    /// value it found nothing in can only have held nothing but null.
    pub(crate) fn unreadable(reading: ValueReading, text: &str) -> AttributeRefusal {
        let required = match reading {
            ValueReading::Temporal(_) => return AttributeRefusal::UnreadableTimestamp { text: Box::from(text) },
            ValueReading::Boolean => RequiredValue::Boolean,
            ValueReading::Integer => RequiredValue::Integer,
            ValueReading::Float => RequiredValue::Float,
            ValueReading::Object => RequiredValue::JsonObject,
            ValueReading::Json => RequiredValue::Json,
            ValueReading::Geometry(_) => RequiredValue::Geometry,
            ValueReading::Identifier(IdentifierReading::Iri) => RequiredValue::Iri,
            ValueReading::Identifier(IdentifierReading::Urn) => RequiredValue::Urn,
            ValueReading::Text => RequiredValue::NonNull,
        };

        AttributeRefusal::unusable(required, text)
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute::refusal::AttributeRefusal, required_value::RequiredValue};
    use cassiopeia_geometry::{error::GeometryError, target::GeometryTarget};
    use cassiopeia_mapping::conversion::value_reading::{IdentifierReading, ValueReading};
    use cassiopeia_ngsi_ld::value::{
        json_payload_error::{JsonPayloadError, JsonShape},
        types::TemporalValueType,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_geometry_error_converts_into_a_refusal() {
        assert_eq!(
            AttributeRefusal::from(GeometryError::GeometryCollection),
            AttributeRefusal::Geometry(GeometryError::GeometryCollection)
        );
    }

    #[test]
    fn an_unusable_refusal_quotes_the_text_it_was_met_with() {
        assert_eq!(
            AttributeRefusal::unusable(RequiredValue::Json, "{not json"),
            AttributeRefusal::UnusableValue {
                required: RequiredValue::Json,
                text: Box::from("{not json"),
            }
        );
    }

    #[test]
    fn a_json_payload_error_converts_into_a_refusal() {
        let error = JsonPayloadError::NotAnObject { found: JsonShape::Number };

        assert_eq!(AttributeRefusal::from(error), AttributeRefusal::JsonPayload(error));
    }

    #[test]
    fn each_reading_refuses_under_what_it_requires() {
        for (reading, required) in [
            (ValueReading::Boolean, RequiredValue::Boolean),
            (ValueReading::Integer, RequiredValue::Integer),
            (ValueReading::Float, RequiredValue::Float),
            (ValueReading::Object, RequiredValue::JsonObject),
            (ValueReading::Json, RequiredValue::Json),
            (ValueReading::Geometry(GeometryTarget::Preserve), RequiredValue::Geometry),
            (ValueReading::Identifier(IdentifierReading::Iri), RequiredValue::Iri),
            (ValueReading::Identifier(IdentifierReading::Urn), RequiredValue::Urn),
            (ValueReading::Text, RequiredValue::NonNull),
        ] {
            assert_eq!(
                AttributeRefusal::unreadable(reading, "x"),
                AttributeRefusal::unusable(required, "x"),
                "{reading:?}"
            );
        }
    }

    #[test]
    fn a_temporal_reading_refuses_as_an_unreadable_timestamp() {
        assert_eq!(
            AttributeRefusal::unreadable(ValueReading::Temporal(TemporalValueType::Date), "nope"),
            AttributeRefusal::UnreadableTimestamp { text: Box::from("nope") }
        );
    }
}
