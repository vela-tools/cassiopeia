use crate::entity::{context_document_error::ContextDocumentError, name::NameBuf};
use jsonld_syntax::{Context, ContextEntry, TryFromJson, Value as JsonLdValue, context::Definition};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use url::Url;

/// One JSON-LD `@context` entry, as ETSI GS CIM 009 v1.9.1 (clause 4.4) and JSON-LD 1.1 (clause
/// 9.15) allow it: `null`, a remote context document, or an inline context definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum NgsiLdContextEntry {
    /// `null`, which clears the active context of every definition before it.
    Null,
    /// A URL pointing at a remote context document.
    Remote(Url),
    /// An inline context definition: term definitions, simple or expanded, and the context-level
    /// keywords such as `@version`, `@protected`, and `@vocab`. Boxed because a definition is
    /// several times the size of the other entries.
    Inline(Box<Definition>),
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

    /// Reads the `@context` member of a JSON-LD context document.
    ///
    /// The member is checked against the whole JSON-LD 1.1 context grammar, so expanded term
    /// definitions, `@version`, `@protected`, and terms mapped to `null` are all read, and kept as
    /// written.
    ///
    /// # Errors
    ///
    /// Returns [`ContextDocumentError`] when the text is not JSON, has no top-level `@context`
    /// member, holds a context that is not valid JSON-LD 1.1, or references a remote context by a
    /// relative reference.
    pub fn from_document(text: &str) -> Result<NgsiLdContext, ContextDocumentError> {
        let JsonValue::Object(mut members) = serde_json::from_str(text)? else {
            return Err(ContextDocumentError::ContextMemberMissing);
        };
        let member = members.remove("@context").ok_or(ContextDocumentError::ContextMemberMissing)?;
        let context = Context::try_from_json(&JsonLdValue::from(member))?;
        NgsiLdContext::try_from(context)
    }

    /// The remote URL when this context is a single remote entry (suitable for a Link header).
    #[must_use]
    pub const fn as_url(&self) -> Option<&Url> {
        match self {
            NgsiLdContext::Single(NgsiLdContextEntry::Remote(url)) => Some(url),
            NgsiLdContext::Single(NgsiLdContextEntry::Null | NgsiLdContextEntry::Inline(_)) | NgsiLdContext::List(_) => None,
        }
    }
}

impl TryFrom<Context> for NgsiLdContext {
    type Error = ContextDocumentError;

    fn try_from(context: Context) -> Result<NgsiLdContext, ContextDocumentError> {
        match context {
            Context::One(entry) => NgsiLdContextEntry::try_from(entry).map(NgsiLdContext::Single),
            Context::Many(entries) => entries
                .into_iter()
                .map(NgsiLdContextEntry::try_from)
                .collect::<Result<_, _>>()
                .map(NgsiLdContext::List),
        }
    }
}

impl TryFrom<ContextEntry> for NgsiLdContextEntry {
    type Error = ContextDocumentError;

    fn try_from(entry: ContextEntry) -> Result<NgsiLdContextEntry, ContextDocumentError> {
        match entry {
            ContextEntry::Null => Ok(NgsiLdContextEntry::Null),
            ContextEntry::IriRef(reference) => match Url::parse(reference.as_str()) {
                Ok(url) => Ok(NgsiLdContextEntry::Remote(url)),
                Err(source) => Err(ContextDocumentError::RemoteNotAbsolute { reference, source }),
            },
            ContextEntry::Definition(definition) => Ok(NgsiLdContextEntry::Inline(Box::new(definition))),
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
    use crate::entity::{
        context::{NgsiLdContext, NgsiLdContextEntry},
        context_document_error::ContextDocumentError,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use url::Url;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn read_and_write_back(document: &JsonValue) -> JsonValue {
        let context = NgsiLdContext::from_document(&document.to_string()).unwrap();
        serde_json::to_value(&context).unwrap()
    }

    #[test]
    fn a_single_remote_context_exposes_its_url() {
        let context = NgsiLdContext::remote(url("https://example.org/ctx.jsonld"));
        assert_eq!(context.as_url(), Some(&url("https://example.org/ctx.jsonld")));
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

    #[test]
    fn a_document_of_plain_string_terms_is_written_back_as_read() {
        let context = json!({"expectedArrival": "https://example.org/expectedArrival"});
        assert_eq!(read_and_write_back(&json!({"@context": context})), context);
    }

    #[test]
    fn a_document_with_an_expanded_term_definition_is_written_back_as_read() {
        let context = json!({
            "expectedArrival": {"@id": "https://example.org/expectedArrival", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}
        });
        assert_eq!(read_and_write_back(&json!({"@context": context})), context);
    }

    #[test]
    fn a_document_with_version_protected_container_and_a_null_term_is_written_back_as_read() {
        let context = json!({
            "@version": 1.1,
            "@protected": true,
            "@vocab": "https://example.org/vocab#",
            "tags": {"@id": "https://example.org/tags", "@container": "@set"},
            "unused": null
        });
        assert_eq!(read_and_write_back(&json!({"@context": context})), context);
    }

    #[test]
    fn a_document_mixing_null_a_remote_url_and_a_definition_keeps_every_entry_in_order() {
        let context = json!([
            null,
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
            {"hasModel": {"@id": "https://example.org/hasModel", "@type": "@id"}}
        ]);
        assert_eq!(read_and_write_back(&json!({"@context": context})), context);
    }

    #[test]
    fn a_document_that_is_not_json_is_unparsable() {
        let error = NgsiLdContext::from_document("{\"@context\": ").unwrap_err();
        assert!(matches!(error, ContextDocumentError::Unparsable(_)));
    }

    #[test]
    fn a_document_without_an_at_context_member_is_rejected() {
        let error = NgsiLdContext::from_document("{\"expectedArrival\": \"https://example.org/expectedArrival\"}").unwrap_err();
        assert!(matches!(error, ContextDocumentError::ContextMemberMissing));
    }

    #[test]
    fn a_document_that_is_not_an_object_is_rejected_as_missing_its_context() {
        let error = NgsiLdContext::from_document("[\"https://example.org/ctx.jsonld\"]").unwrap_err();
        assert!(matches!(error, ContextDocumentError::ContextMemberMissing));
    }

    #[test]
    fn a_term_definition_outside_the_json_ld_grammar_is_invalid() {
        let error = NgsiLdContext::from_document("{\"@context\": {\"expectedArrival\": 42}}").unwrap_err();
        assert!(matches!(error, ContextDocumentError::Invalid(_)));
    }

    #[test]
    fn a_relative_remote_reference_is_rejected() {
        let error = NgsiLdContext::from_document("{\"@context\": \"ctx.jsonld\"}").unwrap_err();
        assert!(matches!(error, ContextDocumentError::RemoteNotAbsolute { .. }));
    }
}
