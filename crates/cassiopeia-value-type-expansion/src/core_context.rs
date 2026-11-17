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

/// Where the W3C Data Integrity v2 `@context` is published, which the core `@context` imports for its
/// `ngsildproof` term.
pub const DATA_INTEGRITY_CONTEXT: Iri<&'static str> = iri!("https://w3id.org/security/data-integrity/v2");

/// The Data Integrity v2 `@context` document as published at [`DATA_INTEGRITY_CONTEXT`].
///
/// The core `@context` defines `ngsildproof` with a scoped `@context` that imports this one, and the
/// JSON-LD 1.1 Create Term Definition algorithm processes a scoped `@context` when it defines the term,
/// failing with an invalid scoped context error when it cannot be loaded. Every expansion against the
/// core `@context` therefore loads it, whether or not the expanded document uses `ngsildproof`, so it
/// is bundled for the same reason the core `@context` is, and its content is fixed for the version its
/// URL names.
const DATA_INTEGRITY_CONTEXT_DOCUMENT: &str = include_str!("../contexts/data-integrity-v2.jsonld");

/// The IRI the core `@context` maps the `valueType` member to, `ngsi-ld:hasValueType` expanded.
pub const HAS_VALUE_TYPE: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/hasValueType");

/// The bundled documents the core `@context` is processed with, each beside the URL it is served
/// under in place of fetching it: the core `@context` and the Data Integrity v2 `@context` it imports.
///
/// # Errors
/// Returns the parse failure when a bundled document is not JSON, which a test rules out.
pub fn core_context_documents() -> Result<[(Iri<&'static str>, RemoteDocument); 2], serde_json::Error> {
    Ok([
        (CORE_CONTEXT, bundled_document(CORE_CONTEXT, CORE_CONTEXT_DOCUMENT)?),
        (
            DATA_INTEGRITY_CONTEXT,
            bundled_document(DATA_INTEGRITY_CONTEXT, DATA_INTEGRITY_CONTEXT_DOCUMENT)?,
        ),
    ])
}

/// The bundled `text` as a loaded remote document served from `url`.
fn bundled_document(url: Iri<&'static str>, text: &str) -> Result<RemoteDocument, serde_json::Error> {
    let document: JsonValue = serde_json::from_str(text)?;

    Ok(RemoteDocument::from_serde_json(Some(url.into()), None, document))
}

#[cfg(test)]
mod tests {
    use crate::core_context::{DATA_INTEGRITY_CONTEXT, core_context_documents};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    #[test]
    fn every_bundled_document_is_served_under_the_url_it_is_bundled_for() {
        for (url, document) in core_context_documents().unwrap() {
            assert_eq!(document.url().map(IriBuf::as_str), Some(url.as_str()));
        }
    }

    #[test]
    fn the_bundled_core_context_coerces_value_type_to_a_vocabulary_iri() {
        let [(_, core), _] = core_context_documents().unwrap();
        let document = core.into_serde_json().into_document();

        assert_eq!(
            document["@context"]["valueType"],
            serde_json::json!({"@id": "ngsi-ld:hasValueType", "@type": "@vocab"})
        );
    }

    #[test]
    fn the_core_context_imports_the_bundled_data_integrity_context_for_ngsildproof() {
        let [(_, core), _] = core_context_documents().unwrap();
        let document = core.into_serde_json().into_document();

        assert_eq!(document["@context"]["ngsildproof"]["@context"][1], DATA_INTEGRITY_CONTEXT.as_str());
    }

    #[test]
    fn the_bundled_data_integrity_context_defines_the_proof_type_the_core_context_relies_on() {
        let [_, (_, data_integrity)] = core_context_documents().unwrap();
        let document = data_integrity.into_serde_json().into_document();

        assert_eq!(
            document["@context"]["DataIntegrityProof"]["@id"],
            "https://w3id.org/security#DataIntegrityProof"
        );
    }
}
