use strum::{Display, EnumCount, EnumIter};

/// Why an entity's attribute values could not be extracted from its source record.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum ExtractorCode {
    /// An attribute's template could not be evaluated against the record.
    TemplateUnresolvable,
    /// The mapping's nested attributes recursed past the guard depth.
    RecursionLimitExceeded,
    /// An attribute declared a temporal transformation and its source carried text that reads as no
    /// supported spelling of a date-time, so the attribute was dropped.
    TimestampUnreadable,
    /// An attribute's source value held nulls, which NGSI-LD admits nowhere in an entity, so the
    /// attribute was written without them.
    NullDropped,
    /// A `JsonProperty`'s source was no JSON object or array of objects, so the attribute was
    /// dropped.
    JsonPayloadMisshapen,
    /// A `VocabProperty`'s source was no IRI, so the attribute was dropped.
    VocabNotAnIri,
    /// A `LanguageProperty`'s source was no map of text keyed by language tag, so the attribute was
    /// dropped.
    LanguageMapMisshapen,
    /// An attribute declared the `json` transformation and its source carried text that is not
    /// JSON, so the attribute was dropped.
    JsonTextUnreadable,
    /// An attribute declared the `object` transformation and its source carried no JSON object,
    /// either as a value or as JSON text, so the attribute was dropped.
    ObjectMisshapen,
    /// An attribute read as a geometry, a `GeoProperty` or one declaring a geometry transformation,
    /// carried a value that spells no `GeoJSON` geometry, so the attribute was dropped.
    GeometryUnreadable,
    /// An attribute declared the `boolean` transformation and its source carried a value that spells
    /// no boolean, so the attribute was dropped.
    BooleanUnreadable,
    /// An attribute declared the `integer` or `float` transformation and its source carried a value
    /// that spells no number of that kind, so the attribute was dropped.
    NumberUnreadable,
}

#[cfg(test)]
mod tests {
    use crate::code::extractor_code::ExtractorCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_extractor_code_renders_a_kebab_case_token() {
        assert_eq!(ExtractorCode::TemplateUnresolvable.to_string(), "template-unresolvable");
        assert_eq!(ExtractorCode::TimestampUnreadable.to_string(), "timestamp-unreadable");
        assert_eq!(ExtractorCode::NullDropped.to_string(), "null-dropped");
        assert_eq!(ExtractorCode::JsonPayloadMisshapen.to_string(), "json-payload-misshapen");
        assert_eq!(ExtractorCode::VocabNotAnIri.to_string(), "vocab-not-an-iri");
        assert_eq!(ExtractorCode::LanguageMapMisshapen.to_string(), "language-map-misshapen");
        assert_eq!(ExtractorCode::JsonTextUnreadable.to_string(), "json-text-unreadable");
        assert_eq!(ExtractorCode::ObjectMisshapen.to_string(), "object-misshapen");
        assert_eq!(ExtractorCode::GeometryUnreadable.to_string(), "geometry-unreadable");
        assert_eq!(ExtractorCode::BooleanUnreadable.to_string(), "boolean-unreadable");
        assert_eq!(ExtractorCode::NumberUnreadable.to_string(), "number-unreadable");
    }
}
