use crate::settings::filter_setting::FilterSetting;
use derive_more::{Display, Into};
use serde::Serialize;
use thiserror::Error;

/// An `OData` `$filter` expression that selects the entities of one collection a read lists, such as
/// `ObservedProperty/name eq 'pm10'` over Datastreams (OGC 18-088 section 9.3.3.5).
///
/// The expression is passed to the server as written, so Cassiopeia checks only what every server
/// needs: a blank filter states nothing to select by, and a control character has no place in a
/// request URL. A filter is written for the collection it selects from, since a Datastream has an
/// `ObservedProperty` where a `MultiDatastream` has `ObservedProperties`, so each collection is
/// selected by a setting of its own, which a refusal names.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Into, Serialize)]
pub struct EntityFilter(Box<str>);

/// Why a text is no filter of the setting that selects the entities of one collection.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EntityFilterError {
    /// The text is empty or only whitespace.
    #[error("a `{}` filter is an OData expression, such as `{}`, not blank text", .0, .0.example())]
    Blank(FilterSetting),

    /// The text holds a control character, such as a line break.
    #[error("a `{0}` filter is one line without control characters")]
    ControlCharacter(FilterSetting),
}

impl EntityFilter {
    /// The filter `text` of the setting `setting`.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityFilterError`] that names `setting` and why `text` is no filter: it is
    /// blank, or it holds a control character.
    pub fn new(setting: FilterSetting, text: String) -> Result<EntityFilter, EntityFilterError> {
        if text.trim().is_empty() {
            return Err(EntityFilterError::Blank(setting));
        }
        if text.chars().any(char::is_control) {
            return Err(EntityFilterError::ControlCharacter(setting));
        }
        Ok(EntityFilter(text.into_boxed_str()))
    }

    /// The expression as written.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::{
        entity_filter::{EntityFilter, EntityFilterError},
        filter_setting::FilterSetting,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_filter_keeps_its_expression_as_written() {
        let filter = EntityFilter::new(FilterSetting::Datastreams, "ObservedProperty/name eq 'pm10'".to_owned()).unwrap();

        assert_eq!(filter.as_str(), "ObservedProperty/name eq 'pm10'");
        assert_eq!(serde_json::to_string(&filter).unwrap(), r#""ObservedProperty/name eq 'pm10'""#);
    }

    #[test]
    fn blank_text_and_control_characters_are_refused() {
        for (text, refusal) in [
            ("", EntityFilterError::Blank(FilterSetting::Datastreams)),
            ("  \t ", EntityFilterError::Blank(FilterSetting::Datastreams)),
            ("name eq 'a'\nor name eq 'b'", EntityFilterError::ControlCharacter(FilterSetting::Datastreams)),
        ] {
            assert_eq!(EntityFilter::new(FilterSetting::Datastreams, text.to_owned()), Err(refusal), "{text:?}");
        }
    }

    #[test]
    fn a_refusal_names_its_setting() {
        assert_eq!(
            EntityFilterError::Blank(FilterSetting::Datastreams).to_string(),
            "a `datastreams` filter is an OData expression, such as `ObservedProperty/name eq 'pm10'`, not blank text"
        );
        assert_eq!(
            EntityFilterError::Blank(FilterSetting::MultiDatastreams).to_string(),
            "a `multiDatastreams` filter is an OData expression, such as `startswith(name,'ws-')`, not blank text"
        );
        assert_eq!(
            EntityFilterError::Blank(FilterSetting::Things).to_string(),
            "a `things` filter is an OData expression, such as `startswith(name,'ILM_')`, not blank text"
        );
        assert_eq!(
            EntityFilterError::ControlCharacter(FilterSetting::MultiDatastreams).to_string(),
            "a `multiDatastreams` filter is one line without control characters"
        );
    }
}
