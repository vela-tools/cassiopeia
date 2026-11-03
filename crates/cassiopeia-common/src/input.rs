use crate::{environment::template::escape, error::input::InputError, masked_url::MaskedUrl};
use derive_more::Display;
use serde::{Serialize, Serializer};
use std::{path::PathBuf, str::FromStr};
use url::Url;

/// A data source, which is either a file on this machine or something to be fetched over the
/// network.
///
/// A `file:` URL is normalised into `Local` at parse time, so a consumer never has to consider a
/// remote variant that is secretly local.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
pub enum Input {
    /// A path on the local filesystem.
    #[display("{}", _0.display())]
    Local(PathBuf),

    /// A source to be downloaded before it can be read, shown with any value it read from the
    /// environment masked.
    #[display("{_0}")]
    Remote(MaskedUrl),
}

impl FromStr for Input {
    type Err = InputError;

    fn from_str(value: &str) -> Result<Input, Self::Err> {
        // A bare Windows path or a relative path is not a URL, so a parse failure means "local"
        // rather than "malformed".
        match Url::parse(value) {
            Ok(url) if url.scheme() == "file" => url.to_file_path().map(Input::Local).map_err(|()| InputError::UnusableFileUrl { url }),
            Ok(url) => Ok(Input::Remote(MaskedUrl::plain(url))),
            Err(_) => Ok(Input::Local(PathBuf::from(value))),
        }
    }
}

// Manual because the wire form is the single string a manifest spells the source as: the template
// it reads back from, with any secret as `${NAME}` and any literal `${` escaped.
impl Serialize for Input {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Input::Local(path) => serializer.serialize_str(&escape(&path.display().to_string())),
            Input::Remote(url) => serializer.serialize_str(&url.template()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::input::Input;
    use pretty_assertions::assert_eq;
    use std::{path::PathBuf, str::FromStr};

    #[test]
    fn an_http_url_parses_as_a_remote_source() {
        let input = Input::from_str("https://example.org/stations.json").unwrap();

        assert!(matches!(input, Input::Remote(_)));
        assert_eq!(input.to_string(), "https://example.org/stations.json");
    }

    #[test]
    fn a_relative_path_parses_as_a_local_source() {
        assert_eq!(Input::from_str("data/stations.csv").unwrap(), Input::Local(PathBuf::from("data/stations.csv")));
    }

    #[test]
    fn a_file_url_is_normalised_into_a_local_path() {
        assert_eq!(
            Input::from_str("file:///tmp/stations.csv").unwrap(),
            Input::Local(PathBuf::from("/tmp/stations.csv"))
        );
    }

    #[test]
    fn a_local_path_with_a_literal_placeholder_opening_serializes_escaped() {
        let encoded = serde_json::to_string(&Input::Local(PathBuf::from("data/${raw}.csv"))).unwrap();

        assert_eq!(encoded, r#""data/$${raw}.csv""#);
    }

    #[test]
    fn a_remote_source_serializes_as_its_url() {
        let input = Input::from_str("https://example.org/stations.json").unwrap();

        assert_eq!(serde_json::to_string(&input).unwrap(), r#""https://example.org/stations.json""#);
    }
}
