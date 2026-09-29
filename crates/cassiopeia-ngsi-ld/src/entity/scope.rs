use crate::entity::error::{NgsiLdError, Result};
use derive_more::Display;
use lazy_regex::regex_is_match;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, str::FromStr};

/// A validated NGSI-LD scope path (ETSI GS CIM 009 v1.9.1, clause 4.18).
///
/// The wire form is the plain scope string; construction rejects any value that is not a legal scope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(try_from = "String", into = "String")]
pub struct ScopeBuf(String);

impl ScopeBuf {
    /// Builds a scope, rejecting any value that is not a legal NGSI-LD scope.
    ///
    /// Each level starts with a Unicode letter and continues with Unicode letters, numbers, or
    /// underscores, matching the `ScopeLevel` grammar of clause 4.18; `urn:ngsi-ld:null` is the one
    /// other legal value.
    ///
    /// # Errors
    /// Returns [`NgsiLdError::InvalidScope`] when `s` is not a legal scope.
    pub fn new(s: impl Into<String>) -> Result<ScopeBuf> {
        let s = s.into();
        if regex_is_match!(r"^(?:/?\p{L}[\p{L}\p{N}_]*(?:/\p{L}[\p{L}\p{N}_]*)*|urn:ngsi-ld:null)$", &s) {
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

/// Whether [`NgsiLdScope::merge`] added any scope that was not already present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeMerge {
    /// Every incoming scope was already present; the set is unchanged.
    Unchanged,
    /// At least one incoming scope was added.
    Extended,
}

/// The scopes an entity belongs to (ETSI GS CIM 009 v1.9.1, clause 4.18).
///
/// Clause 4.18 allows at most one `scope` member per entity, holding one scope or several, so the
/// type is a set: never empty and free of duplicates. Scope membership has no order in the spec, and
/// the fragments of one entity can arrive from concurrent inputs in any order, so the set is kept
/// sorted: the same scopes always serialize the same way, however they were collected.
///
/// On the wire a single scope is a bare string and several are a JSON array, as clause 4.18
/// mandates ("represented as a JSON array in case there is more than one Scope"). Both shapes are
/// accepted when reading; an empty array is rejected because it would declare no scope at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ScopeWire", into = "ScopeWire")]
pub struct NgsiLdScope(BTreeSet<ScopeBuf>);

impl NgsiLdScope {
    /// Builds a scope set from any number of scopes, dropping duplicates.
    ///
    /// Returns `None` when `scopes` yields nothing, because an entity with no scope carries no
    /// `scope` member rather than an empty one.
    #[must_use]
    pub fn from_scopes(scopes: impl IntoIterator<Item = ScopeBuf>) -> Option<NgsiLdScope> {
        let set: BTreeSet<ScopeBuf> = scopes.into_iter().collect();
        if set.is_empty() { None } else { Some(NgsiLdScope(set)) }
    }

    /// Adds every scope of `other` that is not already present.
    ///
    /// This is the merge clause 4.18 mandates when several representations of one entity are
    /// combined, and the same union Append Attributes applies to `scope` without overwrite (clause
    /// 5.6.3).
    pub fn merge(&mut self, other: NgsiLdScope) -> ScopeMerge {
        let before = self.0.len();
        self.0.extend(other.0);
        if self.0.len() == before {
            ScopeMerge::Unchanged
        } else {
            ScopeMerge::Extended
        }
    }

    /// The scopes in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &ScopeBuf> {
        self.0.iter()
    }
}

// `derive_more::From` on this newtype would convert from the inner set, which could be empty; a
// single scope is the one infallible way in, so the conversion is written out.
impl From<ScopeBuf> for NgsiLdScope {
    fn from(scope: ScopeBuf) -> NgsiLdScope {
        NgsiLdScope(BTreeSet::from([scope]))
    }
}

/// The two JSON shapes clause 4.18 gives the `scope` member.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum ScopeWire {
    /// Exactly one scope, written as a bare string.
    Single(ScopeBuf),
    /// Several scopes, written as an array.
    List(Vec<ScopeBuf>),
}

// Choosing the wire shape depends on how many scopes the set holds, which no derive can express.
impl From<NgsiLdScope> for ScopeWire {
    fn from(scope: NgsiLdScope) -> ScopeWire {
        let mut scopes = scope.0.into_iter();
        match (scopes.next(), scopes.len()) {
            (Some(only), 0) => ScopeWire::Single(only),
            (first, _) => ScopeWire::List(first.into_iter().chain(scopes).collect()),
        }
    }
}

// Reading must enforce the non-empty invariant that the untagged wire shape cannot, so the
// conversion is written out.
impl TryFrom<ScopeWire> for NgsiLdScope {
    type Error = NgsiLdError;

    fn try_from(wire: ScopeWire) -> Result<NgsiLdScope> {
        match wire {
            ScopeWire::Single(scope) => Ok(NgsiLdScope::from(scope)),
            ScopeWire::List(scopes) => NgsiLdScope::from_scopes(scopes).ok_or(NgsiLdError::EmptyScopeList),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::scope::{NgsiLdScope, ScopeBuf, ScopeMerge};
    use serde_json::json;

    fn scope(text: &str) -> ScopeBuf {
        ScopeBuf::new(text).unwrap()
    }

    fn scopes(texts: &[&str]) -> NgsiLdScope {
        NgsiLdScope::from_scopes(texts.iter().map(|text| scope(text))).unwrap()
    }

    #[test]
    fn a_hierarchical_scope_is_accepted_and_round_trips() {
        let scope: ScopeBuf = serde_json::from_str("\"/Madrid/Gardens\"").unwrap();
        assert_eq!(scope.as_str(), "/Madrid/Gardens");
        assert_eq!(serde_json::to_string(&scope).unwrap(), "\"/Madrid/Gardens\"");
    }

    #[test]
    fn the_spec_examples_are_accepted() {
        for text in [
            "/Madrid",
            "Madrid",
            "/Madrid/Gardens/ParqueNorte",
            "/CompanyA/OrganizationB/UnitC",
            "urn:ngsi-ld:null",
        ] {
            assert!(ScopeBuf::new(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn a_scope_level_may_use_unicode_letters_and_numbers() {
        assert!(ScopeBuf::new("/Ljubljana/Šiška/Četrt٣").is_ok());
        assert!(ScopeBuf::new("/東京/渋谷_2").is_ok());
    }

    #[test]
    fn a_malformed_scope_is_rejected() {
        for text in ["//bad//", "", "/", "/Madrid/", "/1Madrid", "/_Madrid", "/Madrid Centro", "/Madrid-Centro"] {
            assert!(ScopeBuf::new(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_single_scope_serializes_as_a_bare_string() {
        assert_eq!(serde_json::to_value(scopes(&["/a"])).unwrap(), json!("/a"));
    }

    #[test]
    fn several_scopes_serialize_as_a_sorted_array() {
        assert_eq!(serde_json::to_value(scopes(&["/b", "/a"])).unwrap(), json!(["/a", "/b"]));
    }

    #[test]
    fn a_bare_string_and_an_array_both_deserialize() {
        assert_eq!(serde_json::from_value::<NgsiLdScope>(json!("/a")).unwrap(), scopes(&["/a"]));
        assert_eq!(serde_json::from_value::<NgsiLdScope>(json!(["/a", "/b"])).unwrap(), scopes(&["/a", "/b"]));
    }

    #[test]
    fn a_one_element_array_is_written_back_as_a_bare_string() {
        let read: NgsiLdScope = serde_json::from_value(json!(["/a"])).unwrap();

        assert_eq!(serde_json::to_value(read).unwrap(), json!("/a"));
    }

    #[test]
    fn an_empty_array_is_rejected() {
        assert!(serde_json::from_value::<NgsiLdScope>(json!([])).is_err());
    }

    #[test]
    fn an_array_holding_an_illegal_scope_is_rejected() {
        assert!(serde_json::from_value::<NgsiLdScope>(json!(["/a", "//bad"])).is_err());
    }

    #[test]
    fn duplicate_scopes_collapse_to_one() {
        assert_eq!(serde_json::to_value(scopes(&["/b", "/a", "/b"])).unwrap(), json!(["/a", "/b"]));
    }

    #[test]
    fn no_scopes_yield_no_scope_set() {
        assert!(NgsiLdScope::from_scopes([]).is_none());
    }

    #[test]
    fn merging_adds_only_the_scopes_not_already_present() {
        let mut merged = scopes(&["/c", "/b"]);

        assert_eq!(merged.merge(scopes(&["/b", "/a"])), ScopeMerge::Extended);
        assert_eq!(serde_json::to_value(&merged).unwrap(), json!(["/a", "/b", "/c"]));
    }

    #[test]
    fn merging_a_subset_reports_no_change() {
        let mut merged = scopes(&["/a", "/b"]);

        assert_eq!(merged.merge(scopes(&["/b"])), ScopeMerge::Unchanged);
        assert_eq!(serde_json::to_value(&merged).unwrap(), json!(["/a", "/b"]));
    }

    #[test]
    fn equality_ignores_collection_order() {
        assert_eq!(scopes(&["/a", "/b"]), scopes(&["/b", "/a"]));
    }
}
