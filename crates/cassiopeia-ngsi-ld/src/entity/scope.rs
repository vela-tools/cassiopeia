use crate::entity::error::{NgsiLdError, Result};
use derive_more::Display;
use lazy_regex::regex_is_match;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// A validated NGSI-LD scope path (ETSI GS CIM 009 v1.9.1, clause 4.18).
///
/// The wire form is the plain scope string; construction rejects any value that is not a legal scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(try_from = "String", into = "String")]
pub struct ScopeBuf(String);

impl ScopeBuf {
    /// Builds a scope, rejecting any value that is not a legal NGSI-LD scope.
    ///
    /// # Errors
    /// Returns [`NgsiLdError::InvalidScope`] when `s` is not a legal scope.
    pub fn new(s: impl Into<String>) -> Result<ScopeBuf> {
        let s = s.into();
        if regex_is_match!(r"^(/?([A-Za-z][A-Za-z0-9_]*)(/([A-Za-z][A-Za-z0-9_]*))*|urn:ngsi-ld:null)$", &s) {
            Ok(ScopeBuf(s))
        } else {
            Err(NgsiLdError::InvalidScope { rejected: s.into() })
        }
    }

    /// The scope as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ScopeBuf {
    type Err = NgsiLdError;

    fn from_str(s: &str) -> Result<ScopeBuf> {
        ScopeBuf::new(s)
    }
}

impl TryFrom<String> for ScopeBuf {
    type Error = NgsiLdError;

    fn try_from(s: String) -> Result<ScopeBuf> {
        ScopeBuf::new(s)
    }
}

impl From<ScopeBuf> for String {
    fn from(scope: ScopeBuf) -> String {
        scope.0
    }
}

/// An entity's scope: either a single path or a list of them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum NgsiLdScope {
    /// A single scope path.
    Single(ScopeBuf),
    /// A list of scope paths.
    List(Vec<ScopeBuf>),
}

#[cfg(test)]
mod tests {
    use crate::entity::scope::{NgsiLdScope, ScopeBuf};

    #[test]
    fn a_hierarchical_scope_is_accepted_and_round_trips() {
        let scope: ScopeBuf = serde_json::from_str("\"/Madrid/Gardens\"").unwrap();
        assert_eq!(scope.as_str(), "/Madrid/Gardens");
        assert_eq!(serde_json::to_string(&scope).unwrap(), "\"/Madrid/Gardens\"");
    }

    #[test]
    fn a_malformed_scope_is_rejected() {
        assert!(ScopeBuf::new("//bad//").is_err());
    }

    #[test]
    fn a_scope_can_be_a_single_value_or_a_list() {
        assert!(matches!(serde_json::from_str::<NgsiLdScope>("\"/a\"").unwrap(), NgsiLdScope::Single(_)));
        assert!(matches!(serde_json::from_str::<NgsiLdScope>("[\"/a\", \"/b\"]").unwrap(), NgsiLdScope::List(_)));
    }
}
