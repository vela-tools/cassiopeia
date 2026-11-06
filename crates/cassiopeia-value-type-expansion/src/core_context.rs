use iri_rs::{Iri, iri};
use jsonld::RemoteDocument;
use serde_json::Value as JsonValue;

/// Where the NGSI-LD v1.9 core `@context` is published (ETSI GS CIM 009 v1.9.1 clause 4.4).
pub const CORE_CONTEXT: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld");

/// The core `@context` document as published at [`CORE_CONTEXT`].
///
/// Clause 4.4 makes the core `@context` implicitly present whenever context information is processed,
/// and protected from change, so it is bundled rather than fetched: every expansion needs it, and its
/// content is fixed for the version it is published under.
const CORE_CONTEXT_DOCUMENT: &str = include_str!("../contexts/ngsi-ld-core-context-v1.9.jsonld");

/// The IRI the core `@context` maps the `valueType` member to, `ngsi-ld:hasValueType` expanded.
pub const HAS_VALUE_TYPE: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/hasValueType");

/// The core `@context` as a loaded remote document, served in place of fetching [`CORE_CONTEXT`].
///
/// # Errors
/// Returns the parse failure when the bundled document is not JSON, which a test rules out.
pub fn core_context_document() -> Result<RemoteDocument, serde_json::Error> {
    let document: JsonValue = serde_json::from_str(CORE_CONTEXT_DOCUMENT)?;

    Ok(RemoteDocument::from_serde_json(Some(CORE_CONTEXT.into()), None, document))
}

#[cfg(test)]
mod tests {
    use crate::core_context::{CORE_CONTEXT, core_context_document};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_bundled_core_context_is_served_under_its_published_url() {
        let document = core_context_document().unwrap();

        assert_eq!(document.url().map(IriBuf::as_str), Some(CORE_CONTEXT.as_str()));
    }

    #[test]
    fn the_bundled_core_context_coerces_value_type_to_a_vocabulary_iri() {
        let document = core_context_document().unwrap().into_serde_json().into_document();

        assert_eq!(
            document["@context"]["valueType"],
            serde_json::json!({"@id": "ngsi-ld:hasValueType", "@type": "@vocab"})
        );
    }
}
