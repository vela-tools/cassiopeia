use crate::error::input::InputError;
use derive_more::Display;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};
use std::{path::PathBuf, str::FromStr};
use url::Url;

/// A data source, which is either a file on this machine or something to be fetched over the
/// network.
///
/// A `file:` URL is normalised into `Local` at parse time, so a consumer never has to consider a
/// remote variant that is secretly local.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum Input {
    /// A path on the local filesystem.
    #[display("{}", _0.display())]
    Local(PathBuf),

    /// A source to be downloaded before it can be read.
    #[display("{_0}")]
    Remote(Url),
}

impl FromStr for Input {
    type Err = InputError;

    fn from_str(value: &str) -> Result<Input, Self::Err> {
        // A bare Windows path or a relative path is not a URL, so a parse failure means "local"
        // rather than "malformed".
        match Url::parse(value) {
            Ok(url) if url.scheme() == "file" => url.to_file_path().map(Input::Local).map_err(|()| InputError::UnusableFileUrl { url }),
            Ok(url) => Ok(Input::Remote(url)),
            Err(_) => Ok(Input::Local(PathBuf::from(value))),
        }
    }
}

// Manual because the wire form is the single `Display` string, not the enum's variant structure.
impl Serialize for Input {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

// Manual for the same reason as `Serialize`: the wire form is a single string.
impl<'de> Deserialize<'de> for Input {
    fn deserialize<D>(deserializer: D) -> Result<Input, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Input::from_str(&value).map_err(DeError::custom)
    }
}

#[cfg(test)]
mod tests {
    use crate::input::Input;
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
    fn a_remote_source_round_trips_through_json() {
        let input = Input::from_str("https://example.org/stations.json").unwrap();
        let encoded = serde_json::to_string(&input).unwrap();

        assert_eq!(encoded, r#""https://example.org/stations.json""#);
        assert_eq!(serde_json::from_str::<Input>(&encoded).unwrap(), input);
    }
}
