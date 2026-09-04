use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::PathBuf;
use url::Url;

/// Determines how `@context` is attached to produced entities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtContextMode {
    /// No `@context` is added.
    None,
    /// A user-provided context URL.
    Explicit(Url),
    /// Resolve the context URL from locally downloaded Smart Data Models.
    Default,
    /// Read a local `.jsonld` file and inline its `@context` value into every entity.
    Local(PathBuf),
}

impl Serialize for AtContextMode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AtContextMode::None => serializer.serialize_str("none"),
            AtContextMode::Explicit(url) => serializer.serialize_str(url.as_str()),
            AtContextMode::Default => serializer.serialize_str("default"),
            AtContextMode::Local(path) => serializer.serialize_str(&path.to_string_lossy()),
        }
    }
}

impl<'de> Deserialize<'de> for AtContextMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(parse_context_mode(&s))
    }
}

/// Parses a flat string into an [`AtContextMode`], the form a manifest carries it in.
///
/// - `"none"` becomes [`AtContextMode::None`].
/// - `"default"` becomes [`AtContextMode::Default`].
/// - a `.jsonld` value on an `http`/`https` URL becomes [`AtContextMode::Explicit`], any other
///   `.jsonld` value becomes [`AtContextMode::Local`].
/// - any other value that parses as a URL becomes [`AtContextMode::Explicit`], and everything else
///   becomes [`AtContextMode::Local`].
fn parse_context_mode(s: &str) -> AtContextMode {
    match s {
        "none" => AtContextMode::None,
        "default" => AtContextMode::Default,
        other => {
            if other.ends_with(".jsonld") {
                if let Ok(url) = Url::parse(other) {
                    let is_web_url = url.scheme() == "http" || url.scheme() == "https";
                    if is_web_url {
                        return AtContextMode::Explicit(url);
                    }
                }
                AtContextMode::Local(PathBuf::from(other))
            } else if let Ok(url) = Url::parse(other) {
                AtContextMode::Explicit(url)
            } else {
                AtContextMode::Local(PathBuf::from(other))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::context::mode::{AtContextMode, parse_context_mode};
    use std::path::PathBuf;
    use url::Url;

    #[test]
    fn each_variant_serializes_to_its_flat_string() {
        assert_eq!(serde_json::to_string(&AtContextMode::None).unwrap(), r#""none""#);
        assert_eq!(serde_json::to_string(&AtContextMode::Default).unwrap(), r#""default""#);

        let url = Url::parse("https://example.org/ctx.jsonld").unwrap();
        assert_eq!(
            serde_json::to_string(&AtContextMode::Explicit(url)).unwrap(),
            r#""https://example.org/ctx.jsonld""#
        );

        assert_eq!(
            serde_json::to_string(&AtContextMode::Local(PathBuf::from("./ctx.jsonld"))).unwrap(),
            r#""./ctx.jsonld""#
        );
    }

    #[test]
    fn the_reserved_tokens_deserialize_to_their_variants() {
        assert_eq!(serde_json::from_str::<AtContextMode>(r#""none""#).unwrap(), AtContextMode::None);
        assert_eq!(serde_json::from_str::<AtContextMode>(r#""default""#).unwrap(), AtContextMode::Default);
    }

    #[test]
    fn a_web_jsonld_url_parses_as_an_explicit_context() {
        let expected = Url::parse("https://example.org/ctx.jsonld").unwrap();
        assert_eq!(parse_context_mode("https://example.org/ctx.jsonld"), AtContextMode::Explicit(expected));
    }

    #[test]
    fn a_local_jsonld_path_parses_as_a_local_context() {
        assert_eq!(parse_context_mode("./ctx.jsonld"), AtContextMode::Local(PathBuf::from("./ctx.jsonld")));
    }

    #[test]
    fn a_bare_url_parses_as_an_explicit_context() {
        let expected = Url::parse("http://x/ctx").unwrap();
        assert_eq!(parse_context_mode("http://x/ctx"), AtContextMode::Explicit(expected));
    }

    #[test]
    fn a_bare_word_parses_as_a_local_context() {
        assert_eq!(parse_context_mode("foo"), AtContextMode::Local(PathBuf::from("foo")));
    }
}
