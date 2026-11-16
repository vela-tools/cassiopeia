use crate::stream_kind::StreamKind;
use derive_more::{Display, Into};
use serde::Serialize;
use thiserror::Error;

/// An `OData` `$filter` expression that selects the data streams of one kind a read reads, such as
/// `ObservedProperty/name eq 'pm10'` over Datastreams (OGC 18-088 section 9.3.3.5).
///
/// The expression is passed to the server as written, so Cassiopeia checks only what every server
/// needs: a blank filter states nothing to select by, and a control character has no place in a
/// request URL. A filter is written for the collection it selects from, since a Datastream has an
/// `ObservedProperty` where a `MultiDatastream` has `ObservedProperties`, so each kind is selected by
/// a setting of its own, which a refusal names.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Into, Serialize)]
pub struct StreamFilter(Box<str>);

/// Why a text is no filter of the setting that selects one kind of data stream.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StreamFilterError {
    /// The text is empty or only whitespace.
    #[error("a `{}` filter is an OData expression, such as `{}`, not blank text", .0.setting(), .0.filter_example())]
    Blank(StreamKind),

    /// The text holds a control character, such as a line break.
    #[error("a `{}` filter is one line without control characters", .0.setting())]
    ControlCharacter(StreamKind),
}

impl StreamFilter {
    /// The filter `text` over the data streams of `kind`.
    ///
    /// # Errors
    ///
    /// Returns the [`StreamFilterError`] that names the setting of `kind` and why `text` is no
    /// filter: it is blank, or it holds a control character.
    pub fn new(kind: StreamKind, text: String) -> Result<StreamFilter, StreamFilterError> {
        if text.trim().is_empty() {
            return Err(StreamFilterError::Blank(kind));
        }
        if text.chars().any(char::is_control) {
            return Err(StreamFilterError::ControlCharacter(kind));
        }
        Ok(StreamFilter(text.into_boxed_str()))
    }

    /// The expression as written.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        settings::stream_filter::{StreamFilter, StreamFilterError},
        stream_kind::StreamKind,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_filter_keeps_its_expression_as_written() {
        let filter = StreamFilter::new(StreamKind::Datastream, "ObservedProperty/name eq 'pm10'".to_owned()).unwrap();

        assert_eq!(filter.as_str(), "ObservedProperty/name eq 'pm10'");
        assert_eq!(serde_json::to_string(&filter).unwrap(), r#""ObservedProperty/name eq 'pm10'""#);
    }

    #[test]
    fn blank_text_and_control_characters_are_refused() {
        for (text, refusal) in [
            ("", StreamFilterError::Blank(StreamKind::Datastream)),
            ("  \t ", StreamFilterError::Blank(StreamKind::Datastream)),
            ("name eq 'a'\nor name eq 'b'", StreamFilterError::ControlCharacter(StreamKind::Datastream)),
        ] {
            assert_eq!(StreamFilter::new(StreamKind::Datastream, text.to_owned()), Err(refusal), "{text:?}");
        }
    }

    #[test]
    fn a_refusal_names_the_setting_of_its_kind() {
        assert_eq!(
            StreamFilterError::Blank(StreamKind::Datastream).to_string(),
            "a `datastreams` filter is an OData expression, such as `ObservedProperty/name eq 'pm10'`, not blank text"
        );
        assert_eq!(
            StreamFilterError::Blank(StreamKind::MultiDatastream).to_string(),
            "a `multiDatastreams` filter is an OData expression, such as `startswith(name,'ws-')`, not blank text"
        );
        assert_eq!(
            StreamFilterError::ControlCharacter(StreamKind::MultiDatastream).to_string(),
            "a `multiDatastreams` filter is one line without control characters"
        );
    }
}
