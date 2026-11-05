//! Turns Tera's failure to render a template against one record into the [`ResolutionFailure`] that
//! says what the author should change.
//!
//! The usual cause is a field the record does not have, or has as null. A top-level field the
//! template prints is named by Tera itself. A nested one it prints is named only by its last
//! segment, and one that reaches an operator or a filter not at all: Tera reports the value's kind
//! (`undefined`, `none`) but not the field it came from. Those are found by checking the field
//! paths the template reads against the record. That hint is only given when Tera's report names a
//! missing field or that kind of value, so a field that is merely absent elsewhere in the template
//! is not blamed for an unrelated failure.
//!
//! A missing field is named by the shortest leading part of its path the record does not have,
//! such as `address.street` for `address.street.name` over an `address` without a `street`: Tera
//! reads a missing last segment as undefined, so a guard or a default on that part is one Tera
//! accepts, where one on the whole path would fail on the same missing `street`.

use crate::template::{
    engine_report::EngineReport,
    error::ResolutionFailure,
    field_path::FieldPath,
    field_reads::field_reads,
    registered_template::RegisteredTemplate,
    resolver::RECORD_VARIABLE,
};
use serde_json::Value as JsonValue;
use tera::Tera;

/// Explains why Tera failed to render `template` against `data`.
pub(crate) fn diagnose(tera: &Tera, template: &RegisteredTemplate, data: &JsonValue, engine: tera::Error) -> ResolutionFailure {
    let source = EngineReport::unlocated(engine);
    // The failure outlives the record and the mapping the template is borrowed from.
    let text = template.source.clone();
    if let Some(field) = source.undefined_variable().map(FieldPath::new) {
        return ResolutionFailure::MissingField { template: text, field, source };
    }
    if !data.is_object() {
        return ResolutionFailure::Render { template: text, source };
    }
    let paths = field_reads(template.source.as_str(), &read_variables(tera, template));
    let mut missing = paths.iter().filter_map(|path| path.missing_prefix(data));

    let found = match source.undefined_field() {
        Some(segment) => missing.find(|prefix| prefix.last_segment() == segment),
        None if source.mentions_undefined_value() => missing.next(),
        None => None,
    };
    if let Some(field) = found {
        return ResolutionFailure::MissingField { template: text, field, source };
    }
    if source.mentions_null_value()
        && let Some(field) = paths.into_iter().find(|path| path.lookup(data).is_some_and(JsonValue::is_null))
    {
        return ResolutionFailure::NullField { template: text, field, source };
    }

    ResolutionFailure::Render { template: text, source }
}

/// The top-level variables `template` reads from the record, the record variable aside.
fn read_variables<'a>(tera: &'a Tera, template: &RegisteredTemplate) -> Vec<&'a str> {
    tera.get_template_variables(template.name.as_str())
        .map(|variables| variables.into_iter().filter(|&variable| variable != RECORD_VARIABLE).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::{
        template::{
            TemplateSource,
            error::ResolutionFailure,
            registered_template::RegisteredTemplate,
            render_diagnosis::diagnose,
            template_name::TemplateName,
        },
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use serde_json::{Value as JsonValue, json};
    use std::{path::Path, sync::Arc};
    use tera::{Context, Tera};

    /// The diagnosis of rendering `source`, registered as written, against `data`.
    fn diagnosis(source: &str, data: &JsonValue) -> ResolutionFailure {
        let template = RegisteredTemplate {
            name: TemplateName::for_source(source),
            source: TemplateSource::new(source),
            location: TemplateLocation::new(Arc::from(Path::new("sensor.json5")), TemplateSite::EntityName),
        };
        let mut tera = Tera::default();
        tera.add_raw_template(template.name.as_str(), source).unwrap();
        let mut context = Context::new();
        for (key, value) in data.as_object().unwrap() {
            context.insert(key.clone(), value);
        }
        context.insert("this", data);
        let engine = tera.render(template.name.as_str(), &context).unwrap_err();

        diagnose(&tera, &template, data, engine)
    }

    #[test]
    fn a_field_missing_from_the_record_is_named() {
        assert!(matches!(
            diagnosis("{{ n + 1 }}", &json!({"id": "a"})),
            ResolutionFailure::MissingField { field, .. } if field.as_str() == "n"
        ));
        assert!(matches!(
            diagnosis("{{ missing | upper }}", &json!({"id": "a"})),
            ResolutionFailure::MissingField { field, .. } if field.as_str() == "missing"
        ));
    }

    #[test]
    fn a_missing_field_the_template_prints_is_named() {
        assert!(matches!(
            diagnosis("{{ nothere }}-{{ id | lower }}", &json!({"id": "A"})),
            ResolutionFailure::MissingField { field, .. } if field.as_str() == "nothere"
        ));
    }

    #[test]
    fn a_missing_field_the_template_prints_is_named_over_one_it_only_guards() {
        assert!(matches!(
            diagnosis("{% if absent %}{{ absent }}{% endif %}{{ printed }}", &json!({"id": "A"})),
            ResolutionFailure::MissingField { field, .. } if field.as_str() == "printed"
        ));
    }

    #[test]
    fn a_null_field_reaching_a_filter_is_named() {
        assert!(matches!(
            diagnosis("{{ code | split(pat=' ') }}", &json!({"code": null})),
            ResolutionFailure::NullField { field, .. } if field.as_str() == "code"
        ));
    }

    #[test]
    fn a_failure_on_a_present_value_is_not_blamed_on_another_field() {
        assert!(matches!(
            diagnosis("{% if absent %}{{ absent }}{% endif %}{{ t | int + 1 }}", &json!({"t": "x"})),
            ResolutionFailure::Render { .. }
        ));
    }

    /// The field a diagnosis names as missing, or `None` when it names none.
    fn missing_field(source: &str, data: &JsonValue) -> Option<String> {
        match diagnosis(source, data) {
            ResolutionFailure::MissingField { field, .. } => Some(field.to_string()),
            ResolutionFailure::NullField { .. } | ResolutionFailure::Render { .. } | ResolutionFailure::Decode { .. } => None,
        }
    }

    #[test]
    fn a_missing_nested_field_the_template_prints_is_named_by_its_whole_path() {
        assert_eq!(
            missing_field("{{ a.x }}-{{ id | lower }}", &json!({"id": "A", "a": {"y": 1}})).as_deref(),
            Some("a.x")
        );
    }

    #[test]
    fn a_missing_nested_field_is_named_down_to_the_first_segment_the_record_lacks() {
        assert_eq!(
            missing_field("{{ a.b.c }}-{{ id | lower }}", &json!({"id": "A", "a": {}})).as_deref(),
            Some("a.b")
        );
        assert_eq!(
            missing_field("{{ a.b.c }}-{{ id | lower }}", &json!({"id": "A", "a": {"b": {}}})).as_deref(),
            Some("a.b.c")
        );
    }

    #[test]
    fn a_nested_field_beneath_a_value_that_is_not_an_object_is_named_as_missing() {
        assert_eq!(
            missing_field("{{ a.x }}-{{ id | lower }}", &json!({"id": "A", "a": null})).as_deref(),
            Some("a.x")
        );
        assert_eq!(
            missing_field("{{ a.x }}-{{ id | lower }}", &json!({"id": "A", "a": "s"})).as_deref(),
            Some("a.x")
        );
    }

    #[test]
    fn a_missing_nested_field_reaching_a_filter_or_an_operator_is_named() {
        assert_eq!(missing_field("{{ a.x | upper }}", &json!({"a": {"y": 1}})).as_deref(), Some("a.x"));
        assert_eq!(missing_field("{{ a.x + 1 }}", &json!({"a": {"y": 1}})).as_deref(), Some("a.x"));
    }

    #[test]
    fn a_missing_nested_field_read_through_the_record_variable_is_named_beneath_it() {
        assert_eq!(
            missing_field("{{ this.a.x }}-{{ id | lower }}", &json!({"id": "A", "a": {}})).as_deref(),
            Some("a.x")
        );
    }

    #[test]
    fn the_missing_nested_field_the_template_prints_is_named_over_one_it_only_guards() {
        assert_eq!(
            missing_field("{% if a.y %}{{ a.y }}{% endif %}{{ a.x }}-{{ id | lower }}", &json!({"id": "A", "a": {}})).as_deref(),
            Some("a.x")
        );
    }

    #[test]
    fn a_null_nested_field_reaching_a_filter_is_named() {
        assert!(matches!(
            diagnosis("{{ a.x | upper }}", &json!({"a": {"x": null}})),
            ResolutionFailure::NullField { field, .. } if field.as_str() == "a.x"
        ));
    }

    #[test]
    fn a_failed_bracketed_lookup_names_no_field() {
        assert!(matches!(
            diagnosis("{{ a[0] }}-{{ id | lower }}", &json!({"id": "A", "a": []})),
            ResolutionFailure::Render { .. }
        ));
    }
}
