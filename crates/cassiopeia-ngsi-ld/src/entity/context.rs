use crate::entity::name::NameBuf;
use compact_str::CompactString;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

/// One supported JSON-LD `@context` entry: a remote context document, or an inline map of terms to
/// string definitions.
///
/// Models the two shapes ETSI GS CIM 009 v1.9.1 (clause 4.4) allows an entry to take.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum NgsiLdContextEntry {
    /// A URL pointing at a remote context document.
    Remote(Url),
    /// An inline object mapping terms to string definitions.
    Inline(IndexMap<CompactString, String>),
}

/// A JSON-LD `@context`: either a single entry, or an ordered array of entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum NgsiLdContext {
    /// A single context entry.
    Single(NgsiLdContextEntry),
    /// An ordered list of context entries, applied left to right.
    List(Vec<NgsiLdContextEntry>),
}

impl NgsiLdContext {
    /// Builds a context from a single remote URL.
    #[must_use]
    pub const fn remote(url: Url) -> NgsiLdContext {
        NgsiLdContext::Single(NgsiLdContextEntry::Remote(url))
    }

    /// The remote URL when this context is a single remote entry (suitable for a Link header).
    #[must_use]
    pub const fn as_url(&self) -> Option<&Url> {
        match self {
            NgsiLdContext::Single(NgsiLdContextEntry::Remote(url)) => Some(url),
            NgsiLdContext::Single(NgsiLdContextEntry::Inline(_)) | NgsiLdContext::List(_) => None,
        }
    }

    /// Appends a remote context, promoting a single entry to a list and de-duplicating.
    pub fn add_remote(&mut self, url: Url) {
        let entry = NgsiLdContextEntry::Remote(url);
        match self {
            NgsiLdContext::Single(existing) => {
                if *existing != entry {
                    *self = NgsiLdContext::List(vec![existing.clone(), entry]);
                }
            }
            NgsiLdContext::List(list) => {
                if !list.contains(&entry) {
                    list.push(entry);
                }
            }
        }
    }
}

/// How `@context` is resolved and attached to entities.
#[derive(Debug, Clone)]
pub enum ContextSource {
    /// No `@context` is attached.
    None,
    /// A single context applied to every entity.
    Static(NgsiLdContext),
    /// A per-entity-type context, keyed by entity type name.
    PerEntityType(HashMap<NameBuf, NgsiLdContext>),
}

impl ContextSource {
    /// Looks up the context for a given entity type.
    #[must_use]
    pub fn resolve(&self, entity_type: &NameBuf) -> Option<&NgsiLdContext> {
        match self {
            ContextSource::None => None,
            ContextSource::Static(context) => Some(context),
            ContextSource::PerEntityType(map) => map.get(entity_type),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::context::{NgsiLdContext, NgsiLdContextEntry};
    use url::Url;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn a_single_remote_context_exposes_its_url() {
        let context = NgsiLdContext::remote(url("https://example.org/ctx.jsonld"));
        assert_eq!(context.as_url(), Some(&url("https://example.org/ctx.jsonld")));
    }

    #[test]
    fn adding_a_second_remote_promotes_to_a_list_and_no_longer_reads_as_a_single_url() {
        let mut context = NgsiLdContext::remote(url("https://example.org/a.jsonld"));
        context.add_remote(url("https://example.org/b.jsonld"));

        assert!(matches!(context, NgsiLdContext::List(ref list) if list.len() == 2));
        assert_eq!(context.as_url(), None);
    }

    #[test]
    fn a_url_string_deserializes_as_a_single_remote_entry() {
        let context: NgsiLdContext = serde_json::from_str("\"https://example.org/ctx.jsonld\"").unwrap();
        assert!(matches!(context, NgsiLdContext::Single(NgsiLdContextEntry::Remote(_))));
    }

    #[test]
    fn an_array_of_urls_deserializes_as_a_list() {
        let context: NgsiLdContext = serde_json::from_str("[\"https://example.org/a.jsonld\", \"https://example.org/b.jsonld\"]").unwrap();
        assert!(matches!(context, NgsiLdContext::List(_)));
    }

    #[test]
    fn an_inline_object_deserializes_as_an_inline_entry() {
        let context: NgsiLdContext = serde_json::from_str("{\"temp\": \"https://example.org/temperature\"}").unwrap();
        assert!(matches!(context, NgsiLdContext::Single(NgsiLdContextEntry::Inline(_))));
    }
}
