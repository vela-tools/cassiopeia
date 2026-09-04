use crate::entity::{
    attribute::NestedAttributes,
    representation::{SerializeRepr, concise, normalized, simplified},
};
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use chrono::{DateTime, Utc};
use foldhash::fast::RandomState;
use indexmap::IndexMap;
use langtag::LangTagBuf;
use serde::{Deserialize, Serialize, Serializer};
use urn_rs::Urn;

/// A `languageMap`: localized text keyed by BCP-47 language tag (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.18).
///
/// The tags come from the mapping's own `languageMap` declaration, so the map hashes with `foldhash`
/// rather than the standard library's `SipHash`.
pub type LanguageMap = IndexMap<LangTagBuf, String, RandomState>;

/// An NGSI-LD `LanguageProperty` attribute (ETSI GS CIM 009 v1.9.1, clause 4.5.18).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NgsiLdLanguageProperty {
    /// A map of BCP-47 language tag to localized text.
    #[serde(rename = "languageMap")]
    pub language_map: LanguageMap,
    /// When it was observed.
    #[serde(rename = "observedAt", skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// The dataset this instance belongs to (a URI).
    #[serde(rename = "datasetId", skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Urn>,
    /// Nested sub-attributes.
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: NestedAttributes,
}

impl NgsiLdLanguageProperty {
    /// Builds a language property from a map of BCP-47 language tags to text.
    #[must_use]
    pub fn new(language_map: LanguageMap) -> NgsiLdLanguageProperty {
        NgsiLdLanguageProperty {
            language_map,
            observed_at: None,
            dataset_id: None,
            attributes: NestedAttributes::default(),
        }
    }
}

impl SerializeRepr for NgsiLdLanguageProperty {
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error> {
        match representation {
            NgsiLdRepresentation::Normalized => normalized::serialize_language_property(self, skip_null, serializer),
            NgsiLdRepresentation::Concise => concise::serialize_language_property(self, skip_null, serializer),
            NgsiLdRepresentation::Simplified => simplified::serialize_language_property(self, skip_null, serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::attribute::language_property::NgsiLdLanguageProperty;
    use indexmap::IndexMap;
    use langtag::LangTagBuf;

    #[test]
    fn a_new_language_property_holds_its_map() {
        let mut map = IndexMap::default();
        map.insert(LangTagBuf::new("en".to_string()).unwrap(), "Hello".to_string());
        let property = NgsiLdLanguageProperty::new(map);
        assert_eq!(property.language_map.len(), 1);
    }
}
