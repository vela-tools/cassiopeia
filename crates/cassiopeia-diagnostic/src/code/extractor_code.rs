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
    }
}
