use strum::Display;

/// What an NGSI-LD attribute kind needs its value to be, for a kind that refuses a value it cannot
/// hold rather than holding it as text.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
pub enum RequiredValue {
    /// A `VocabProperty`'s `vocab` member names a vocabulary IRI (ETSI GS CIM 009 v1.9.1 clause
    /// 4.5.20).
    #[strum(to_string = "an IRI")]
    Iri,
    /// A `LanguageProperty`'s `languageMap` holds text keyed by BCP-47 language tag (ETSI GS CIM 009
    /// v1.9.1 clause 4.5.18).
    #[strum(to_string = "a language map of text keyed by language tag")]
    LanguageMap,
}

#[cfg(test)]
mod tests {
    use crate::required_value::RequiredValue;
    use pretty_assertions::assert_eq;

    #[test]
    fn each_requirement_reads_as_the_value_it_asks_for() {
        assert_eq!(RequiredValue::Iri.to_string(), "an IRI");
        assert_eq!(RequiredValue::LanguageMap.to_string(), "a language map of text keyed by language tag");
    }
}
