use crate::{
    end_to_end::harness::{RunBinding, RunInput, RunOutcome, STORES, run_csv_end_to_end, run_end_to_end},
    test_reporter::RecordedDiagnostic,
};
use cassiopeia_diagnostic::{
    code::{diagnostic_code::DiagnosticCode, expander_code::ExpanderCode, extractor_code::ExtractorCode, transform_code::TransformCode},
    context_field::ContextField,
};
use cassiopeia_ngsi_ld::entity::{attribute_path::AttributePath, name::NameBuf};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// Three sites: `A` observed at a time that does not read, `B` in a zone that is no legal scope
/// level, and a third with no `site` to name it.
const SITES: &str = "site,zone,a,b,time
A,Wien,1,2,soon
B,Novo mesto,3,4,2026-03-01T11:00:00Z
,Graz,5,6,2026-03-01T11:00:00Z
";

/// Two attributes observed at the same time, one reading it as a date-time and one as written.
const SITE_MAPPING: &str = r#"{
    version: "v4",
    dataModel: "Site",
    identity: { entityName: "{{ site }}" },
    scope: "/{{ zone }}",
    attributes: {
        a: {
            source: "{{ a }}",
            transformation: "integer",
            properties: { observedAt: { source: "{{ time }}", transformation: "datetime" } },
        },
        b: {
            source: "{{ b }}",
            transformation: "integer",
            properties: { observedAt: { source: "{{ time }}" } },
        },
    },
}"#;

/// A site scoped by whether it is indoors and by its area, carrying its area and the time it was
/// last visited as values of their own.
const SCOPED_SITE_MAPPING: &str = r#"{
    version: "v4",
    dataModel: "Site",
    identity: { entityName: "{{ site }}" },
    scope: ["{{ indoor }}", "/{{ area | upper }}"],
    attributes: {
        area: { source: "{{ area }}" },
        when: { source: "{{ when }}", transformation: "datetime" },
    },
}"#;

/// The written entity `urn:ngsi-ld:Site:<site>`.
fn site<'a>(outcome: &'a RunOutcome, site: &str) -> &'a Value {
    let id = format!("urn:ngsi-ld:Site:{site}");
    outcome
        .entities
        .iter()
        .find(|entity| entity["id"] == id.as_str())
        .unwrap_or_else(|| panic!("{id} written: {:?}", outcome.entities))
}

/// The run's one diagnostic under `code`, checked to stand for one occurrence.
fn the_one(outcome: &RunOutcome, code: DiagnosticCode) -> &RecordedDiagnostic {
    let reported = outcome.coded(code);
    assert_eq!(reported.len(), 1, "{code}: {:?}", outcome.diagnostics);
    assert_eq!(reported[0].occurrences, 1, "{code}");
    reported[0]
}

/// The context naming the top-level attribute `name`.
fn attribute(name: &str) -> ContextField {
    ContextField::Attribute(AttributePath::top_level(NameBuf::new(name).unwrap()))
}

#[test]
fn each_reason_a_record_is_skipped_and_each_unreadable_observed_at_is_counted_under_its_own_code() {
    for store in STORES {
        let outcome = run_csv_end_to_end(store, SITES, &RunBinding::Mapping(SITE_MAPPING), &["Site"], "");

        let illegal_scope = &the_one(&outcome, DiagnosticCode::Expander(ExpanderCode::ScopeUnreadable)).headline;
        assert!(illegal_scope.contains("/Novo mesto"), "{store:?}: {illegal_scope}");
        the_one(&outcome, DiagnosticCode::Expander(ExpanderCode::IdAbsent));
        let mut unreadable = outcome.coded(DiagnosticCode::Transform(TransformCode::ObservedAtUnreadable));
        unreadable.sort_by(|left, right| left.headline.cmp(&right.headline));
        let unreadable: Vec<(&[ContextField], u64)> = unreadable
            .into_iter()
            .map(|diagnostic| (diagnostic.defining.as_slice(), diagnostic.occurrences))
            .collect();
        assert_eq!(unreadable, [(&[attribute("a")][..], 1), (&[attribute("b")][..], 1)], "{store:?}");
        assert!(
            outcome.coded(DiagnosticCode::Extractor(ExtractorCode::TimestampUnreadable)).is_empty(),
            "{store:?}"
        );

        let written = site(&outcome, "A");
        for name in ["a", "b"] {
            assert!(written[name]["value"].is_number(), "{store:?}: {written}");
            assert_eq!(written[name].get("observedAt"), None, "{store:?}: {written}");
        }
    }
}

#[test]
fn a_scope_that_fails_to_render_one_that_is_not_text_and_a_value_that_does_not_read_are_counted_apart() {
    let inputs = [RunInput {
        binding: RunBinding::Mapping(SCOPED_SITE_MAPPING),
        records: json!([
            { "site": "A", "when": "2026-03-01T11:00:00Z" },
            { "site": "B", "indoor": true, "area": "north", "when": "2026-03-01T11:00:00Z" },
            { "site": "C", "area": "north", "when": "soon" },
        ]),
    }];
    for store in STORES {
        let outcome = run_end_to_end(store, &inputs, &["Site"], "");

        the_one(&outcome, DiagnosticCode::Expander(ExpanderCode::ScopeUnresolvable));
        let not_text = &the_one(&outcome, DiagnosticCode::Expander(ExpanderCode::ScopeUnreadable)).headline;
        assert!(not_text.contains("reads `indoor`"), "{store:?}: {not_text}");
        let unreadable_value = the_one(&outcome, DiagnosticCode::Extractor(ExtractorCode::TimestampUnreadable));
        assert_eq!(unreadable_value.defining, [attribute("when")], "{store:?}");

        let written = site(&outcome, "C");
        assert_eq!(written["scope"], json!("/NORTH"), "{store:?}: {written}");
        assert_eq!(written["area"]["value"], json!("north"), "{store:?}: {written}");
        assert_eq!(written.get("when"), None, "{store:?}: {written}");
        assert_eq!(outcome.entities.len(), 1, "{store:?}: {:?}", outcome.entities);
    }
}
