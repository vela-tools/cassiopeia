use crate::entity::error::{NgsiLdError, Result};
use compact_str::CompactString;
use derive_more::Display;
use lazy_regex::regex_is_match;
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, str::FromStr};

/// A validated NGSI-LD attribute or entity-type name (ETSI GS CIM 009 v1.9.1, clause 4.6.2).
///
/// The wire form is the plain name string; construction rejects any value that is not a legal name.
///
/// The text is held in a [`CompactString`] because a name is written once and copied constantly: the
/// same handful of attribute names is cloned per attribute, per entity, at every stage that rebuilds
/// an entity's attribute map. NGSI-LD names are short, so they live inline in the value itself and a
/// clone is a fixed-size copy with no allocation at all.
///
/// Sharing the buffer through an `Arc` instead would remove the copy as well, but it is measurably
/// worse here: every pipeline stage runs on its own threads and they all clone the same few names, so
/// the shared refcount becomes a contended cache line and costs more than the allocations it saves.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(try_from = "String")]
pub struct NameBuf(CompactString);

impl NameBuf {
    /// Builds a name, rejecting any value that is not a legal NGSI-LD name.
    ///
    /// # Errors
    /// Returns [`NgsiLdError::InvalidAttributeName`] when `s` is not a legal name.
    pub fn new(s: impl Into<String>) -> Result<NameBuf> {
        let s = s.into();
        if regex_is_match!(r"^\p{L}[\p{L}\p{N}_]*(?::\p{L}[\p{L}\p{N}_]*)?$", &s) {
            Ok(NameBuf(CompactString::from(s)))
        } else {
            Err(NgsiLdError::InvalidAttributeName { rejected: s.into() })
        }
    }

    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for NameBuf {
    type Err = NgsiLdError;

    fn from_str(s: &str) -> Result<NameBuf> {
        NameBuf::new(s)
    }
}

// Lets a `NameBuf`-keyed map be looked up by a plain `&str`. The borrow is the inner string, whose
// `Hash`/`Eq` agree with the derived `Hash`/`Eq` on `NameBuf`, so the map invariant is preserved.
impl Borrow<str> for NameBuf {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NameBuf {
    type Error = NgsiLdError;

    fn try_from(s: String) -> Result<NameBuf> {
        NameBuf::new(s)
    }
}

impl From<NameBuf> for String {
    fn from(name: NameBuf) -> String {
        name.0.into_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::name::NameBuf;
    use std::collections::HashMap;

    #[test]
    fn a_legal_name_is_accepted_and_round_trips() {
        let name: NameBuf = serde_json::from_str("\"temperature\"").unwrap();
        assert_eq!(name.as_str(), "temperature");
        assert_eq!(serde_json::to_string(&name).unwrap(), "\"temperature\"");
    }

    #[test]
    fn a_prefixed_name_is_accepted() {
        assert!(NameBuf::new("schema:temperature").is_ok());
    }

    #[test]
    fn a_name_starting_with_a_digit_is_rejected() {
        assert!(NameBuf::new("1temp").is_err());
        assert!(serde_json::from_str::<NameBuf>("\"1temp\"").is_err());
    }

    #[test]
    fn a_name_keyed_map_can_be_looked_up_by_a_string_slice() {
        let mut map = HashMap::new();
        map.insert(NameBuf::new("observedAt").unwrap(), 1);
        assert_eq!(map.get("observedAt"), Some(&1));
    }

    #[test]
    fn a_cloned_name_compares_equal_by_text() {
        let name = NameBuf::new("temperature").unwrap();
        let copy = name.clone();

        assert_eq!(name, copy);
        assert_eq!(name, NameBuf::new("temperature").unwrap());
        assert_eq!(String::from(copy), "temperature");
    }

    #[test]
    fn a_name_round_trips_through_json_as_a_plain_string() {
        let name = NameBuf::new("schema:temperature").unwrap();
        let encoded = serde_json::to_string(&name).unwrap();

        assert_eq!(encoded, "\"schema:temperature\"");
        assert_eq!(serde_json::from_str::<NameBuf>(&encoded).unwrap(), name);
    }
}
