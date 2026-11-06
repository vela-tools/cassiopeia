use crate::{
    end_to_end::harness::{RunBinding, RunInput, RunOutcome, run_end_to_end, run_failing_end_to_end},
    error::PipelineError,
    local_context_error::LocalContextError,
};
use cassiopeia_common::store_kind::StoreKind;
use cassiopeia_diagnostic::{
    cause::Cause,
    code::{context_code::ContextCode, diagnostic_code::DiagnosticCode},
    diagnostic_builder::from_error,
    severity::Severity,
};
use cassiopeia_ngsi_ld::entity::context_document_error::ContextDocumentError;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{fs, path::Path};
use temp_dir::TempDir;

/// An alert whose `expectedArrival` is read as a date and time.
const ALERT: &str = r#"{
    version: "v4",
    dataModel: "Alert",
    identity: { entityName: "{{ id }}" },
    attributes: {
        expectedArrival: { source: "{{ due }}", transformation: "datetime" },
    },
}"#;

/// One alert record.
fn alert_input() -> RunInput {
    RunInput {
        binding: RunBinding::Mapping(ALERT),
        records: json!([{ "id": "A1", "due": "2026-10-24T20:49:00Z" }]),
    }
}

/// The `output` members that attach the local context file at `path`.
fn local_context(path: &Path) -> String {
    format!(r#", context: "{}""#, path.display())
}

/// Runs the alert mapping with the local context file at `path`, which must fail the run.
fn failing_run(path: &Path) -> (PipelineError, RunOutcome) {
    run_failing_end_to_end(StoreKind::DashMap, &[alert_input()], &["Alert"], &local_context(path))
}

/// The headline and first cause of the diagnostic a failure is reported with, the temporary
/// directory written as `<dir>`.
fn reported(error: &PipelineError, directory: &TempDir) -> (String, Option<String>) {
    let diagnostic = from_error(Severity::Error, DiagnosticCode::from(error), error).build();
    let directory = directory.path().display().to_string();
    (
        diagnostic.headline().replace(&directory, "<dir>"),
        diagnostic.causes().first().map(Cause::as_str).map(str::to_string),
    )
}

#[test]
fn a_local_context_with_expanded_terms_version_and_protected_is_attached_to_every_entity_as_written() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("expanded.jsonld");
    let context = json!([
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
        {
            "@version": 1.1,
            "@protected": true,
            "expectedArrival": { "@id": "https://example.org/expectedArrival", "@type": "https://uri.etsi.org/ngsi-ld/DateTime" },
            "unused": null
        }
    ]);
    fs::write(&path, json!({ "@context": context }).to_string()).unwrap();

    let outcome = run_end_to_end(StoreKind::DashMap, &[alert_input()], &["Alert"], &local_context(&path));

    assert_eq!(outcome.entities.len(), 1);
    assert_eq!(outcome.entities[0].get("@context"), Some(&context));
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, DiagnosticCode::Context(_))),
        "a readable local context reports nothing"
    );
}

#[test]
fn a_local_context_of_plain_string_terms_is_attached_to_every_entity_as_written() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("plain.jsonld");
    let context = json!({ "expectedArrival": "https://example.org/expectedArrival" });
    fs::write(&path, json!({ "@context": context }).to_string()).unwrap();

    let outcome = run_end_to_end(StoreKind::DashMap, &[alert_input()], &["Alert"], &local_context(&path));

    let contexts: Vec<Option<&Value>> = outcome.entities.iter().map(|entity| entity.get("@context")).collect();
    assert_eq!(contexts, vec![Some(&context)]);
}

#[test]
fn a_local_context_outside_the_json_ld_grammar_fails_the_run_before_any_entity_is_written() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("invalid.jsonld");
    fs::write(&path, r#"{"@context": {"expectedArrival": 42}}"#).unwrap();

    let (error, outcome) = failing_run(&path);

    assert!(matches!(
        &error,
        PipelineError::LocalContext(LocalContextError::Unusable {
            source: ContextDocumentError::Invalid(_),
            ..
        })
    ));
    assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Context(ContextCode::ContextUnreadable));
    assert_eq!(
        reported(&error, &directory),
        (
            "Cannot read the @context from '<dir>/invalid.jsonld'".to_string(),
            Some("The '@context' member is not a valid JSON-LD 1.1 context".to_string())
        )
    );
    assert_eq!(outcome.entities, Vec::<Value>::new());
}

#[test]
fn a_local_context_file_without_an_at_context_member_fails_the_run_before_any_entity_is_written() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("bare.jsonld");
    fs::write(&path, r#"{"expectedArrival": "https://example.org/expectedArrival"}"#).unwrap();

    let (error, outcome) = failing_run(&path);

    assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Context(ContextCode::ContextUnreadable));
    assert_eq!(
        reported(&error, &directory),
        (
            "Cannot read the @context from '<dir>/bare.jsonld'".to_string(),
            Some("The document has no top-level '@context' member".to_string())
        )
    );
    assert_eq!(outcome.entities, Vec::<Value>::new());
}

#[test]
fn a_local_context_file_that_is_not_json_fails_the_run_before_any_entity_is_written() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("broken.jsonld");
    fs::write(&path, r#"{"@context": "#).unwrap();

    let (error, outcome) = failing_run(&path);

    assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Context(ContextCode::FileUnparsable));
    assert_eq!(
        reported(&error, &directory),
        (
            "Cannot read the @context from '<dir>/broken.jsonld'".to_string(),
            Some("The document is not valid JSON".to_string())
        )
    );
    assert_eq!(outcome.entities, Vec::<Value>::new());
}

#[test]
fn a_missing_local_context_file_fails_the_run_before_any_entity_is_written() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("absent.jsonld");

    let (error, outcome) = failing_run(&path);

    assert!(matches!(&error, PipelineError::LocalContext(LocalContextError::Unreadable { .. })));
    assert_eq!(DiagnosticCode::from(&error), DiagnosticCode::Context(ContextCode::FileUnreadable));
    assert_eq!(reported(&error, &directory).0, "Cannot read the local @context file '<dir>/absent.jsonld'");
    assert_eq!(outcome.entities, Vec::<Value>::new());
}
