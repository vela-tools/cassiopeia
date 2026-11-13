use derive_more::{Display, Into};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// An `OData` `$filter` expression over Datastreams that selects what a read reads, such as
/// `ObservedProperty/name eq 'pm10'` (OGC 18-088 section 9.3.3.5).
///
/// The expression is passed to the server as written, so Cassiopeia checks only what every server
/// needs: a blank filter states nothing to select by, and a control character has no place in a
/// request URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Into, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct DatastreamFilter(Box<str>);

/// Why a text is no `datastreams` filter.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DatastreamFilterError {
    /// The text is empty or only whitespace.
    #[error("a `datastreams` filter is an OData expression, such as `ObservedProperty/name eq 'pm10'`, not blank text")]
    Blank,

    /// The text holds a control character, such as a line break.
    #[error("a `datastreams` filter is one line without control characters")]
    ControlCharacter,
}

impl DatastreamFilter {
    /// The expression as written.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DatastreamFilter {
    type Error = DatastreamFilterError;

    fn try_from(text: String) -> Result<DatastreamFilter, DatastreamFilterError> {
        if text.trim().is_empty() {
            return Err(DatastreamFilterError::Blank);
        }
        if text.chars().any(char::is_control) {
            return Err(DatastreamFilterError::ControlCharacter);
        }
        Ok(DatastreamFilter(text.into_boxed_str()))
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::datastream_filter::{DatastreamFilter, DatastreamFilterError};
    use pretty_assertions::assert_eq;

    fn filter(text: &str) -> Result<DatastreamFilter, DatastreamFilterError> {
        DatastreamFilter::try_from(text.to_owned())
    }

    #[test]
    fn a_blank_or_multi_line_filter_is_refused() {
        assert_eq!(filter(""), Err(DatastreamFilterError::Blank));
        assert_eq!(filter("   "), Err(DatastreamFilterError::Blank));
        assert_eq!(filter("a\nb"), Err(DatastreamFilterError::ControlCharacter));
        assert_eq!(filter("ObservedProperty/name eq 'pm10'").unwrap().as_str(), "ObservedProperty/name eq 'pm10'");
    }

    #[test]
    fn a_filter_round_trips_as_its_text() {
        let filter: DatastreamFilter = serde_json::from_str(r#""endswith(name,'-pm10')""#).unwrap();

        assert_eq!(serde_json::to_string(&filter).unwrap(), r#""endswith(name,'-pm10')""#);
    }

    #[test]
    fn a_refused_filter_names_why() {
        let error = serde_json::from_str::<DatastreamFilter>(r#""\t""#).unwrap_err();

        assert!(error.to_string().contains("not blank text"), "{error}");
    }
}
