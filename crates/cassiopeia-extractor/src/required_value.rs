use strum::Display;

/// What a declaration needs its value to be, for an NGSI-LD attribute kind that refuses a value it
/// cannot hold rather than holding it as text, or a transformation that refuses a value it cannot
/// read.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
pub enum RequiredValue {
    /// The `boolean` transformation reads a boolean, or the text or number that spells one.
    #[strum(to_string = "a boolean")]
    Boolean,
    /// The `integer` transformation reads a 64-bit signed integer, or the text or number that spells
    /// one.
    #[strum(to_string = "an integer")]
    Integer,
    /// The `float` transformation reads a finite double-precision number, or the text that spells
    /// one.
    #[strum(to_string = "a number")]
    Float,
    /// A `VocabProperty`'s `vocab` member names a vocabulary IRI (ETSI GS CIM 009 v1.9.1 clause
    /// 4.5.20).
    #[strum(to_string = "an IRI")]
    Iri,
    /// A `LanguageProperty`'s `languageMap` holds text keyed by BCP-47 language tag (ETSI GS CIM 009
    /// v1.9.1 clause 4.5.18).
    #[strum(to_string = "a language map of text keyed by language tag")]
    LanguageMap,
    /// The `json` transformation reads text as the JSON value it is written as (RFC 8259 clause 2).
    #[strum(to_string = "JSON")]
    Json,
    /// The `object` transformation keeps a JSON object, or reads text as one (RFC 8259 clause 4).
    #[strum(to_string = "a JSON object")]
    JsonObject,
    /// A geometry transformation, and a `GeoProperty` declaring none, reads a value as the `GeoJSON`
    /// geometry it spells (RFC 7946 clause 3.1), or as the coordinates of the type it names.
    #[strum(to_string = "a GeoJSON geometry")]
    Geometry,
}

#[cfg(test)]
mod tests {
    use crate::required_value::RequiredValue;
    use pretty_assertions::assert_eq;

    #[test]
    fn each_requirement_reads_as_the_value_it_asks_for() {
        assert_eq!(RequiredValue::Boolean.to_string(), "a boolean");
        assert_eq!(RequiredValue::Integer.to_string(), "an integer");
        assert_eq!(RequiredValue::Float.to_string(), "a number");
        assert_eq!(RequiredValue::Iri.to_string(), "an IRI");
        assert_eq!(RequiredValue::LanguageMap.to_string(), "a language map of text keyed by language tag");
        assert_eq!(RequiredValue::Json.to_string(), "JSON");
        assert_eq!(RequiredValue::JsonObject.to_string(), "a JSON object");
        assert_eq!(RequiredValue::Geometry.to_string(), "a GeoJSON geometry");
    }
}
