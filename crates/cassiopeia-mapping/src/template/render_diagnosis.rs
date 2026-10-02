//! Turns Tera's failure to render a template against one record into the [`ResolutionFailure`] that
//! says what the author should change.
//!
//! The usual cause is a field the record does not have, or has as null, reaching an operator or a
//! filter that needs a value. Tera reports the value's kind (`undefined`, `none`) but not the field
//! it came from, so the field is found by checking the variables the template reads against the
//! record. The hint is only given when Tera's report names that kind of value, so a field that is
//! merely absent elsewhere in the template is not blamed for an unrelated failure.

use crate::template::{engine_report::EngineReport, error::ResolutionFailure, field_path::FieldPath, registered_template::RegisteredTemplate};
use serde_json::{Map, Value as JsonValue};
use tera::Tera;

/// The variable the resolver binds to the whole record rather than to one of its fields.
const RECORD_VARIABLE: &str = "this";

/// Explains why Tera failed to render `template` against `data`.
pub(crate) fn diagnose(tera: &Tera, template: &RegisteredTemplate, data: &JsonValue, engine: tera::Error) -> ResolutionFailure {
    let source = EngineReport::unlocated(engine);
    // The failure outlives the record and the mapping the template is borrowed from.
    let text = template.source.clone();
    let Some(fields) = data.as_object() else {
        return ResolutionFailure::Render { template: text, source };
    };
    let variables = read_variables(tera, template);

    if source.mentions_undefined_value()
        && let Some(field) = variables.iter().find(|variable| !fields.contains_key(**variable))
    {
        return ResolutionFailure::MissingField {
            template: text,
            field: FieldPath::new(*field),
            source,
        };
    }
    if source.mentions_null_value()
        && let Some(field) = variables.iter().find(|variable| is_null(fields, variable))
    {
        return ResolutionFailure::NullField {
            template: text,
            field: FieldPath::new(*field),
            source,
        };
    }

    ResolutionFailure::Render { template: text, source }
}

/// The top-level variables `template` reads from the record, in name order so the field a hint
/// names is the same on every run.
fn read_variables<'a>(tera: &'a Tera, template: &RegisteredTemplate) -> Vec<&'a str> {
    let mut variables: Vec<&str> = tera
        .get_template_variables(template.name.as_str())
        .map(|variables| variables.into_iter().filter(|&variable| variable != RECORD_VARIABLE).collect())
        .unwrap_or_default();
    variables.sort_unstable();
    variables
}

/// Whether `field` is present in `fields` and null.
fn is_null(fields: &Map<String, JsonValue>, field: &str) -> bool {
    fields.get(field).is_some_and(JsonValue::is_null)
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
}
