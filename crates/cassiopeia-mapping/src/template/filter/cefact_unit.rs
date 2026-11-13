use crate::template::registrar::Registrar;
use cassiopeia_sensorthings::unit_of_measurement::UnitOfMeasurement;
use cassiopeia_unit_notation::resolution::resolve;
use cefact_units::UnitCode;
use tera::{Error, Kwargs, Map, State, TeraResult, Value, value::Key};

/// Registers the `cefact_unit` filter on a Tera engine.
///
/// NGSI-LD writes a unit as a UN/CEFACT Recommendation No. 20 code in `unitCode` (ETSI GS CIM 009 v1.9.1
/// clause 4.5.2), while sources send symbols, UCUM codes, QUDT unit IRIs, or unit names. `cefact_unit`
/// translates such a notation into its code: `unitCode: { source: "{{ unit | cefact_unit }}" }`.
///
/// A text is resolved as a whole. A map is read as a SensorThings `unitOfMeasurement`, whose `name`,
/// `symbol`, and `definition` members each describe the unit (OGC 18-088 section 8.2.4, Table 10), so the
/// mapping need not choose which one a server fills in correctly: the first of `definition`, `symbol` and
/// `name` that resolves gives the code, as [`UnitOfMeasurement::code`] reads it. A null or absent value
/// yields nothing.
///
/// A notation that resolves to no code passes through unchanged, on purpose: a filter can only return a
/// value or fail its template, and a failing template drops the whole attribute, so the unresolved text
/// is left to reach `unitCode`, where it can be reported by name instead of vanishing here. Of a map, the
/// text passed through is [`UnitOfMeasurement::text`], the one a reader recognises soonest.
pub fn register(registrar: &mut Registrar) {
    registrar.filter("cefact_unit", |value: &Value, _kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
        cefact_unit(value)
    });
}

/// The code `value` denotes, or `value` itself when it denotes none.
fn cefact_unit(value: &Value) -> TeraResult<Value> {
    if let Some(notation) = value.as_str() {
        return Ok(resolve(notation).map_or_else(|| Value::from(notation), code_value));
    }
    if let Some(unit) = value.as_map() {
        return Ok(unit_of_measurement(unit));
    }
    if value.is_none() || value.is_undefined() {
        return Ok(Value::none());
    }
    Err(Error::message(format!(
        "`cefact_unit` reads a unit notation text or a SensorThings unitOfMeasurement object, not {}",
        kind_noun(value)
    )))
}

/// The code `unit` denotes, else its text, else nothing.
fn unit_of_measurement(unit: &Map) -> Value {
    let unit = UnitOfMeasurement::read(|member| unit.get(&Key::Str(member)).and_then(Value::as_str));

    unit.code().map(code_value).or_else(|| unit.text().map(Value::from)).unwrap_or_else(Value::none)
}

/// `code` as the text `unitCode` holds.
fn code_value(code: UnitCode) -> Value {
    Value::from(code.as_str())
}

/// A noun phrase naming the kind of `value`, for the message refusing it.
fn kind_noun(value: &Value) -> &'static str {
    if value.is_number() {
        "a number"
    } else if value.is_bool() {
        "a boolean"
    } else if value.is_array() {
        "an array"
    } else {
        value.name()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        template::{TemplateSource, compile_error::TemplateCompileError, filter::cefact_unit::register, registrar::Registrar, runner::TemplateRunner},
        template_location::TemplateLocation,
        template_site::TemplateSite,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};
    use std::{path::Path, sync::Arc};
    use tera::{Context, Value};

    fn render(unit: &JsonValue) -> Result<String, tera::Error> {
        let mut registrar = Registrar::with_engine_builtins();
        register(&mut registrar);
        let (mut tera, _) = registrar.finish();
        tera.add_raw_template("t", "{{ u | cefact_unit }}").unwrap();

        let mut context = Context::new();
        context.insert_value("u", Value::try_from_serializable(unit).unwrap());

        tera.render("t", &context)
    }

    #[test]
    fn a_known_notation_becomes_its_code() {
        assert_eq!(render(&json!("ug/m3")).unwrap(), "GQ");
        assert_eq!(render(&json!("\u{B5}g/m\u{B3}")).unwrap(), "GQ");
        assert_eq!(render(&json!("https://qudt.org/vocab/unit/MicroGM-PER-M3")).unwrap(), "GQ");
    }

    #[test]
    fn a_qudt_definition_wins_over_an_unknown_symbol() {
        let unit = json!({"definition": "https://qudt.org/vocab/unit/MicroGM-PER-M3", "symbol": "xx"});

        assert_eq!(render(&unit).unwrap(), "GQ");
    }

    #[test]
    fn the_symbol_is_used_when_the_definition_is_a_ucum_anchor() {
        let unit = json!({"definition": "http://unitsofmeasure.org/ucum.html#para-30", "symbol": "\u{B0}C"});

        assert_eq!(render(&unit).unwrap(), "CEL");
    }

    #[test]
    fn the_name_is_used_when_nothing_else_resolves() {
        assert_eq!(render(&json!({"name": "kelvin"})).unwrap(), "KEL");
    }

    #[test]
    fn the_rivm_samen_meten_unit_of_measurement_resolves() {
        let unit = json!({"name": "ug/m3", "symbol": "ug/m3", "definition": "https://qudt.org/vocab/unit/MicroGM-PER-M3"});

        assert_eq!(render(&unit).unwrap(), "GQ");
    }

    #[test]
    fn an_unresolved_object_yields_its_symbol() {
        assert_eq!(render(&json!({"symbol": "xyzzy", "name": "xyzzy per fortnight"})).unwrap(), "xyzzy");
    }

    #[test]
    fn an_unresolved_object_without_symbol_yields_its_name_then_its_definition() {
        assert_eq!(render(&json!({"name": "x"})).unwrap(), "x");
        assert_eq!(render(&json!({"definition": "urn:x"})).unwrap(), "urn:x");
    }

    #[test]
    fn a_member_that_is_not_text_is_ignored() {
        assert_eq!(render(&json!({"symbol": 5, "name": "kelvin"})).unwrap(), "KEL");
    }

    #[test]
    fn an_empty_object_yields_nothing() {
        assert_eq!(render(&json!({})).unwrap(), "");
    }

    #[test]
    fn null_yields_nothing() {
        assert_eq!(render(&JsonValue::Null).unwrap(), "");
    }

    #[test]
    fn an_unknown_text_passes_through_unchanged() {
        assert_eq!(render(&json!("QQQ")).unwrap(), "QQQ");
    }

    #[test]
    fn a_number_fails_the_template_naming_its_kind() {
        let error = render(&json!(5)).unwrap_err();

        assert!(
            format!("{error:?}").contains("`cefact_unit` reads a unit notation text or a SensorThings unitOfMeasurement object, not a number"),
            "{error:?}"
        );
    }

    #[test]
    fn a_boolean_and_an_array_fail_the_template_naming_their_kinds() {
        assert!(format!("{:?}", render(&json!(true)).unwrap_err()).contains("not a boolean"));
        assert!(format!("{:?}", render(&json!(["ug/m3"])).unwrap_err()).contains("not an array"));
    }

    #[test]
    fn a_misspelt_filter_is_answered_with_cefact_unit() {
        let location = TemplateLocation::new(Arc::from(Path::new("air-quality.json5")), TemplateSite::EntityName);
        let error = *TemplateRunner::new()
            .compile(&TemplateSource::new("{{ u | cefact_units }}"), &location)
            .unwrap_err();

        assert!(
            matches!(&error, TemplateCompileError::MisspelledCallable { suggestion: "cefact_unit", .. }),
            "{error}"
        );
    }
}
