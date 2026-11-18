use crate::entity::error::{NgsiLdError, Result};
use derive_more::Display;
use iri_rs::UriBuf;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use urn_rs::Urn;

/// The prefix that names the `urn` scheme, matched without regard to ASCII case because a URI scheme
/// is case-insensitive (RFC 3986 clause 3.1).
const URN_PREFIX: &str = "urn:";

/// The dataset an attribute instance belongs to, as its `datasetId` member names it (ETSI GS CIM 009
/// v1.9.1 clause 4.5.5.1).
///
/// Clause 4.5.5.1 types `datasetId` as a URI, so any absolute URI (RFC 3986) is held, a URN or not.
/// A URI whose scheme is `urn` must also be a URN (RFC 8141) and is held in its normal form: the
/// `urn:` prefix and the namespace identifier lowercased and percent-encodings in uppercase hex, the
/// rest keeping its case, exactly as the `urn` transformation writes a URN. Clause 4.5.5.1 also
/// admits the JSON-LD keyword `@none`, which names the default instance; that instance is written
/// without a `datasetId`, so `@none` is no `DatasetId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(try_from = "String", into = "String")]
pub struct DatasetId(UriBuf);

impl DatasetId {
    /// Builds a dataset id from the text a mapping resolved.
    ///
    /// # Errors
    /// Returns [`NgsiLdError::InvalidDatasetId`] when `text` is no absolute URI, or names the `urn`
    /// scheme and is no URN.
    pub fn new(text: &str) -> Result<DatasetId> {
        let refused = || NgsiLdError::InvalidDatasetId { rejected: text.into() };
        let names_urn = text.get(..URN_PREFIX.len()).is_some_and(|prefix| prefix.eq_ignore_ascii_case(URN_PREFIX));
        let uri = if names_urn {
            let urn = Urn::from_str(text).map_err(|_| refused())?;
            UriBuf::parse(urn.into_string())
        } else {
            UriBuf::from_str(text)
        };

        uri.map(DatasetId).map_err(|_| refused())
    }

    /// The dataset id as it is written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<String> for DatasetId {
    type Error = NgsiLdError;

    fn try_from(text: String) -> Result<DatasetId> {
        DatasetId::new(&text)
    }
}

impl From<DatasetId> for String {
    fn from(dataset_id: DatasetId) -> String {
        dataset_id.0.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::{dataset_id::DatasetId, error::NgsiLdError};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_urn_is_kept_as_written() {
        assert_eq!(DatasetId::new("urn:ngsi-ld:Dataset:gfs").unwrap().as_str(), "urn:ngsi-ld:Dataset:gfs");
    }

    #[test]
    fn a_urn_in_another_case_is_held_in_its_normal_form() {
        assert_eq!(DatasetId::new("URN:NGSI-LD:Dataset:gfs").unwrap().as_str(), "urn:ngsi-ld:Dataset:gfs");
    }

    #[test]
    fn an_absolute_uri_that_is_no_urn_is_kept_as_written() {
        assert_eq!(
            DatasetId::new("https://example.org/datasets/gfs").unwrap().as_str(),
            "https://example.org/datasets/gfs"
        );
    }

    #[test]
    fn text_that_names_no_dataset_is_refused_naming_the_text() {
        for text in ["gfs model", "datasets/gfs", "@none", "", "urn:x"] {
            let error = DatasetId::new(text).unwrap_err();

            assert!(
                matches!(&error, NgsiLdError::InvalidDatasetId { rejected } if rejected.as_ref() == text),
                "{text}: {error:?}"
            );
            assert!(error.to_string().contains(&format!("'{text}'")), "{text}: {error}");
        }
    }

    #[test]
    fn a_dataset_id_round_trips_through_json_as_a_plain_string() {
        let dataset_id: DatasetId = serde_json::from_str(r#""https://example.org/a""#).unwrap();

        assert_eq!(dataset_id, DatasetId::new("https://example.org/a").unwrap());
        assert_eq!(serde_json::to_string(&dataset_id).unwrap(), r#""https://example.org/a""#);
    }

    #[test]
    fn text_that_names_no_dataset_does_not_deserialize() {
        assert!(serde_json::from_str::<DatasetId>(r#""gfs model""#).is_err());
    }
}
