use serde::{Deserialize, Serialize};
use strum::Display;

/// The manifest document version.
///
/// The version is declared by every manifest so the reader can reject a document written against a
/// shape it does not understand rather than silently misreading it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Version {
    /// The first and only manifest version.
    V1,
}

#[cfg(test)]
mod tests {
    use crate::version::Version;

    #[test]
    fn the_wire_form_is_the_lowercase_version_tag() {
        assert_eq!(serde_json::to_string(&Version::V1).unwrap(), r#""v1""#);
        assert_eq!(Version::V1.to_string(), "v1");
    }

    #[test]
    fn an_unknown_version_is_rejected() {
        assert!(serde_json::from_str::<Version>(r#""v2""#).is_err());
    }
}
