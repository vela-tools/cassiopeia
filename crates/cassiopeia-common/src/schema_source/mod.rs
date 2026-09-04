use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{convert::Infallible, path::PathBuf, str::FromStr};
use url::Url;

/// The origin of a custom JSON Schema validation source, given on the command line, in a manifest
/// output, or on a single mapping.
///
/// A source is either a file on this machine or a document to fetch over the network. The two are
/// told apart by sniffing the value: a string that parses as an `http`/`https` [`Url`] is
/// [`Remote`](SchemaSource::Remote); anything else (a bare filename, a relative or absolute path, or
/// a URL with any other scheme) is [`Local`](SchemaSource::Local). This mirrors how
/// [`AtContextMode`](crate::context::mode::AtContextMode) sniffs a `@context` source, so the flag and
/// the file agree on what a given string means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaSource {
    /// A schema file on the local filesystem, resolved relative to whatever declared it.
    Local(PathBuf),

    /// A schema document to download before it can be compiled.
    Remote(Url),
}

impl FromStr for SchemaSource {
    // Sniffing never fails: a value that is not a recognised web URL is taken as a local path, so
    // every input string yields a source. The error type is uninhabited to say so.
    type Err = Infallible;

    fn from_str(value: &str) -> Result<SchemaSource, Self::Err> {
        Ok(parse_schema_source(value))
    }
}

/// Parses a flat string into a [`SchemaSource`], the form the CLI flag and a manifest both carry.
///
/// A string that parses as a URL whose scheme is `http` or `https` becomes
/// [`SchemaSource::Remote`]; every other string (including a URL with a non-web scheme such as
/// `file:`) becomes [`SchemaSource::Local`] over the string verbatim.
fn parse_schema_source(value: &str) -> SchemaSource {
    if let Ok(url) = Url::parse(value)
        && (url.scheme() == "http" || url.scheme() == "https")
    {
        return SchemaSource::Remote(url);
    }

    SchemaSource::Local(PathBuf::from(value))
}

impl Serialize for SchemaSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            SchemaSource::Local(path) => serializer.serialize_str(&path.to_string_lossy()),
            SchemaSource::Remote(url) => serializer.serialize_str(url.as_str()),
        }
    }
}

impl<'de> Deserialize<'de> for SchemaSource {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(parse_schema_source(&value))
    }
}

#[cfg(test)]
mod tests {
    use crate::schema_source::{SchemaSource, parse_schema_source};
    use std::path::PathBuf;
    use url::Url;

    #[test]
    fn a_bare_filename_parses_as_a_local_source() {
        assert_eq!(parse_schema_source("x.schema.json"), SchemaSource::Local(PathBuf::from("x.schema.json")));
    }

    #[test]
    fn a_relative_path_parses_as_a_local_source() {
        assert_eq!(parse_schema_source("./rel/x.json"), SchemaSource::Local(PathBuf::from("./rel/x.json")));
    }

    #[test]
    fn an_https_url_parses_as_a_remote_source() {
        let expected = Url::parse("https://example.org/x.schema.json").unwrap();
        assert_eq!(parse_schema_source("https://example.org/x.schema.json"), SchemaSource::Remote(expected));
    }

    #[test]
    fn an_http_url_parses_as_a_remote_source() {
        let expected = Url::parse("http://example.org/x.schema.json").unwrap();
        assert_eq!(parse_schema_source("http://example.org/x.schema.json"), SchemaSource::Remote(expected));
    }

    #[test]
    fn a_file_url_parses_as_a_local_source() {
        // A non-web scheme is not fetched; the value is kept verbatim as a local path.
        assert_eq!(parse_schema_source("file:///x.json"), SchemaSource::Local(PathBuf::from("file:///x.json")));
    }

    #[test]
    fn a_remote_source_round_trips_through_json() {
        let source = parse_schema_source("https://example.org/x.schema.json");
        let encoded = serde_json::to_string(&source).unwrap();

        assert_eq!(encoded, r#""https://example.org/x.schema.json""#);
        assert_eq!(serde_json::from_str::<SchemaSource>(&encoded).unwrap(), source);
    }

    #[test]
    fn a_local_source_round_trips_through_json() {
        let source = parse_schema_source("schemas/ExoPlanet.json");
        let encoded = serde_json::to_string(&source).unwrap();

        assert_eq!(encoded, r#""schemas/ExoPlanet.json""#);
        assert_eq!(serde_json::from_str::<SchemaSource>(&encoded).unwrap(), source);
    }
}
