pub mod clean;
pub mod elementary;
pub mod get;
pub mod json_decode;
pub mod rounding;
pub mod timestamp;
pub mod trigonometry;

use crate::template::{
    filter::{clean::clean, get::get, timestamp::date_subtract_seconds},
    registrar::Registrar,
};

/// Registers every Cassiopeia-defined filter on a Tera engine.
///
/// Kept in one place so a newly added filter is available to every engine the crate builds,
/// rather than only to whichever construction path happened to be updated.
pub fn register(registrar: &mut Registrar) {
    registrar.filter("clean", clean);
    registrar.filter("get", get);
    registrar.filter("date_subtract_seconds", date_subtract_seconds);
    elementary::register(registrar);
    json_decode::register(registrar);
    rounding::register(registrar);
    trigonometry::register(registrar);
}

#[cfg(test)]
mod tests {
    use crate::template::{filter::register, registrar::Registrar};
    use tera::Context;

    fn render(template: &str) -> String {
        let mut registrar = Registrar::with_engine_builtins();
        register(&mut registrar);
        let (mut tera, _) = registrar.finish();
        tera.add_raw_template("t", template).unwrap();

        tera.render("t", &Context::new()).unwrap()
    }

    #[test]
    fn the_clean_filter_is_registered() {
        assert_eq!(render(r#"{{ "Main   Street " | clean }}"#), "Main Street");
    }

    #[test]
    fn the_date_subtract_seconds_filter_is_registered() {
        assert_eq!(
            render(r#"{{ "2026-04-03T22:00:20Z" | date_subtract_seconds(seconds=20) }}"#),
            "2026-04-03T22:00:00Z"
        );
    }

    #[test]
    fn the_elementary_filters_are_registered() {
        assert_eq!(render("{{ 16 | sqrt }}"), "4.0");
    }

    #[test]
    fn the_json_decode_filter_is_registered() {
        assert_eq!(render(r#"{{ '[{"id": 7}]' | json_decode | first | get(key="id") }}"#), "7");
    }

    #[test]
    fn the_rounding_filters_are_registered() {
        assert_eq!(render("{{ 2.9 | floor }}"), "2.0");
    }

    #[test]
    fn the_trigonometry_filters_are_registered() {
        assert_eq!(render("{{ 0 | cos }}"), "1.0");
    }
}
