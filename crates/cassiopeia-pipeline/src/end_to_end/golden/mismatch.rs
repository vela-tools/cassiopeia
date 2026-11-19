use crate::end_to_end::golden::divergence::{Divergence, Step};
use serde_json::Value;

/// Says where the entities a view wrote first depart from the entities its snapshot stores: the
/// entity, by its `id` and its index in the file, and its first member that differs; or an entity
/// only one side holds.
///
/// Returns `None` when the two are equal.
pub(crate) fn entities_mismatch(stored: &Value, written: &Value) -> Option<String> {
    let divergence = Divergence::between(stored, written)?;
    let mismatch = match divergence.steps() {
        &[Step::Item(index)] => {
            let entity = named(stored, written, "", index, "id");
            match divergence.written() {
                Some(_) => format!("the run wrote entity {entity}, at index {index}, which the snapshot does not hold"),
                None => format!("the run did not write entity {entity}, which the snapshot holds at index {index}"),
            }
        }
        &[Step::Item(index), ..] => format!(
            "entity {}, at index {index}, differs first at `{}`: the snapshot holds {}, the run wrote {}",
            named(stored, written, "", index, "id"),
            divergence.pointer_after(1),
            shown(divergence.stored()),
            shown(divergence.written()),
        ),
        &[] | &[Step::Member(_), ..] => format!(
            "the entities differ first at `{}`: the snapshot holds {}, the run wrote {}",
            divergence.pointer_after(0),
            shown(divergence.stored()),
            shown(divergence.written()),
        ),
    };
    Some(mismatch)
}

/// Says where what a view's runs reported first departs from the report its snapshot stores: the
/// diagnostic, by its code and its index, and its first member that differs, such as its
/// `occurrences` or a line of its `message`; a diagnostic only one side holds; or the failure that
/// ended the run.
///
/// Returns `None` when the two are equal.
pub(crate) fn report_mismatch(stored: &Value, written: &Value) -> Option<String> {
    let divergence = Divergence::between(stored, written)?;
    let mismatch = match divergence.steps() {
        &[Step::Member("diagnostics"), Step::Item(index)] => {
            let diagnostic = named(stored, written, "/diagnostics", index, "code");
            match divergence.written() {
                Some(_) => format!("the run reported diagnostic {diagnostic}, at index {index}, which the snapshot does not hold"),
                None => format!("the run did not report diagnostic {diagnostic}, which the snapshot holds at index {index}"),
            }
        }
        &[Step::Member("diagnostics"), Step::Item(index), ..] => format!(
            "diagnostic {}, at index {index}, differs first at `{}`: the snapshot holds {}, the run reported {}",
            named(stored, written, "/diagnostics", index, "code"),
            divergence.pointer_after(2),
            shown(divergence.stored()),
            shown(divergence.written()),
        ),
        &[] | &[Step::Item(_), ..] | &[Step::Member(_), ..] => format!(
            "the report differs first at `{}`: the snapshot holds {}, the run reported {}",
            divergence.pointer_after(0),
            shown(divergence.stored()),
            shown(divergence.written()),
        ),
    };
    Some(mismatch)
}

/// The item at `index` of the array at the JSON Pointer `list`, named by its text member `key`, in
/// backticks: as the snapshot stores it, or as the run wrote it when the snapshot holds no such
/// item.
fn named(stored: &Value, written: &Value, list: &str, index: usize, key: &str) -> String {
    let pointer = format!("{list}/{index}/{key}");
    stored
        .pointer(&pointer)
        .or_else(|| written.pointer(&pointer))
        .and_then(Value::as_str)
        .map_or_else(|| format!("without `{key}`"), |name| format!("`{name}`"))
}

/// A value as compact JSON in backticks, or `nothing` when a side holds none there.
fn shown(value: Option<&Value>) -> String {
    value.map_or_else(|| String::from("nothing"), |value| format!("`{value}`"))
}

#[cfg(test)]
mod tests {
    use crate::end_to_end::golden::mismatch::{entities_mismatch, report_mismatch};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn equal_entities_and_reports_have_no_mismatch() {
        let entities = json!([{ "id": "urn:ngsi-ld:Place:P1", "type": "Place" }]);
        let report = json!({ "diagnostics": [{ "code": "extractor-null-dropped", "occurrences": 1, "message": ["! a"] }] });

        assert_eq!(entities_mismatch(&entities, &entities), None);
        assert_eq!(report_mismatch(&report, &report), None);
    }

    #[test]
    fn an_entity_member_mismatch_names_the_entity_and_the_member() {
        let stored = json!([
            { "id": "urn:ngsi-ld:Place:P1", "name": { "type": "Property", "value": "Castle" } },
            { "id": "urn:ngsi-ld:Place:P2", "name": { "type": "Property", "value": "Bridge" } },
        ]);
        let written = json!([
            { "id": "urn:ngsi-ld:Place:P1", "name": { "type": "Property", "value": "Castle" } },
            { "id": "urn:ngsi-ld:Place:P2", "name": { "type": "Property", "value": "Bridges" } },
        ]);

        assert_eq!(
            entities_mismatch(&stored, &written).unwrap(),
            "entity `urn:ngsi-ld:Place:P2`, at index 1, differs first at `/name/value`: the snapshot holds `\"Bridge\"`, the run wrote `\"Bridges\"`"
        );
    }

    #[test]
    fn a_member_the_run_left_out_reads_as_nothing_written() {
        let stored = json!([{ "id": "urn:ngsi-ld:Place:P1", "name": "Castle" }]);
        let written = json!([{ "id": "urn:ngsi-ld:Place:P1" }]);

        assert_eq!(
            entities_mismatch(&stored, &written).unwrap(),
            "entity `urn:ngsi-ld:Place:P1`, at index 0, differs first at `/name`: the snapshot holds `\"Castle\"`, the run wrote nothing"
        );
    }

    #[test]
    fn an_entity_only_one_side_holds_is_named_by_its_id() {
        let one = json!([{ "id": "urn:ngsi-ld:Place:P1" }]);
        let two = json!([{ "id": "urn:ngsi-ld:Place:P1" }, { "id": "urn:ngsi-ld:Place:P2" }]);

        assert_eq!(
            entities_mismatch(&one, &two).unwrap(),
            "the run wrote entity `urn:ngsi-ld:Place:P2`, at index 1, which the snapshot does not hold"
        );
        assert_eq!(
            entities_mismatch(&two, &one).unwrap(),
            "the run did not write entity `urn:ngsi-ld:Place:P2`, which the snapshot holds at index 1"
        );
    }

    #[test]
    fn entities_in_another_order_are_named_by_the_stored_id() {
        let stored = json!([{ "id": "urn:ngsi-ld:Place:P1" }, { "id": "urn:ngsi-ld:Place:P2" }]);
        let written = json!([{ "id": "urn:ngsi-ld:Place:P2" }, { "id": "urn:ngsi-ld:Place:P1" }]);

        assert_eq!(
            entities_mismatch(&stored, &written).unwrap(),
            "entity `urn:ngsi-ld:Place:P1`, at index 0, differs first at `/id`: the snapshot holds `\"urn:ngsi-ld:Place:P1\"`, the run wrote `\"urn:ngsi-ld:Place:P2\"`"
        );
    }

    #[test]
    fn an_entity_without_an_id_is_named_as_such() {
        let stored = json!([{ "type": "Place" }]);
        let written = json!([{ "type": "Site" }]);

        assert_eq!(
            entities_mismatch(&stored, &written).unwrap(),
            "entity without `id`, at index 0, differs first at `/type`: the snapshot holds `\"Place\"`, the run wrote `\"Site\"`"
        );
    }

    #[test]
    fn a_diagnostic_count_mismatch_names_the_code() {
        let stored = json!({ "diagnostics": [{ "code": "extractor-number-unreadable", "occurrences": 2, "message": ["! a"] }] });
        let written = json!({ "diagnostics": [{ "code": "extractor-number-unreadable", "occurrences": 3, "message": ["! a"] }] });

        assert_eq!(
            report_mismatch(&stored, &written).unwrap(),
            "diagnostic `extractor-number-unreadable`, at index 0, differs first at `/occurrences`: the snapshot holds `2`, the run reported `3`"
        );
    }

    #[test]
    fn a_diagnostic_message_mismatch_names_the_code_and_the_line() {
        let stored = json!({ "diagnostics": [{ "code": "extractor-null-dropped", "occurrences": 1, "message": ["! a", "  ╰─ attribute   b"] }] });
        let written = json!({ "diagnostics": [{ "code": "extractor-null-dropped", "occurrences": 1, "message": ["! a", "  ╰─ attribute   c"] }] });

        assert_eq!(
            report_mismatch(&stored, &written).unwrap(),
            "diagnostic `extractor-null-dropped`, at index 0, differs first at `/message/1`: the snapshot holds `\"  ╰─ attribute   b\"`, the run reported `\"  ╰─ attribute   c\"`"
        );
    }

    #[test]
    fn a_diagnostic_only_one_side_holds_is_named_by_its_code() {
        let none = json!({ "diagnostics": [] });
        let one = json!({ "diagnostics": [{ "code": "transform-unit-code-unknown", "occurrences": 1, "message": ["! a"] }] });

        assert_eq!(
            report_mismatch(&none, &one).unwrap(),
            "the run reported diagnostic `transform-unit-code-unknown`, at index 0, which the snapshot does not hold"
        );
        assert_eq!(
            report_mismatch(&one, &none).unwrap(),
            "the run did not report diagnostic `transform-unit-code-unknown`, which the snapshot holds at index 0"
        );
    }

    #[test]
    fn a_failure_mismatch_names_the_failure() {
        let stored = json!({ "diagnostics": [] });
        let written = json!({ "failure": ["mapping refused"], "diagnostics": [] });

        assert_eq!(
            report_mismatch(&stored, &written).unwrap(),
            "the report differs first at `/failure`: the snapshot holds nothing, the run reported `[\"mapping refused\"]`"
        );
    }
}
