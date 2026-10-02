use crate::template::registrar::Registrar;
use tera_contrib::{
    base64::{b64_decode, b64_encode},
    dates::{date, is_after, is_before, now},
    filesize_format::filesize_format,
    format::format,
    json::json_encode,
    rand::{get_random, shuffle},
    regex::{Matching, RegexReplace, spaceless, striptags},
    slug::slug,
    urlencode::{urlencode, urlencode_strict},
};

/// Registers the tera-contrib filters, functions, and tests on a Tera engine.
///
/// Tera's core ships without any dependency-bearing built-ins; the `tera-contrib` crate holds
/// them, so dates, slugs, base64, HTML stripping, and the rest have to be registered explicitly
/// before a mapping template can call them. Registration must happen before any template is added,
/// since Tera resolves every referenced filter, function, and test at template-compile time.
pub fn register(registrar: &mut Registrar) {
    registrar.filter("b64_decode", b64_decode);
    registrar.filter("b64_encode", b64_encode);
    registrar.filter("date", date);
    registrar.filter("filesize_format", filesize_format);
    registrar.filter("format", format);
    registrar.filter("json_encode", json_encode);
    registrar.filter("regex_replace", RegexReplace::default());
    registrar.filter("shuffle", shuffle);
    registrar.filter("slug", slug);
    registrar.filter("spaceless", spaceless);
    registrar.filter("striptags", striptags);
    registrar.filter("urlencode", urlencode);
    registrar.filter("urlencode_strict", urlencode_strict);
    registrar.function("get_random", get_random);
    registrar.function("now", now);
    registrar.test("after", is_after);
    registrar.test("before", is_before);
    registrar.test("matching", Matching::default());
}

#[cfg(test)]
mod tests {
    use crate::template::{contrib::register, registrar::Registrar};
    use tera::Context;

    fn render(template: &str) -> String {
        let mut registrar = Registrar::with_engine_builtins();
        register(&mut registrar);
        let (mut tera, _) = registrar.finish();
        tera.add_raw_template("t", template).unwrap();

        tera.render("t", &Context::new()).unwrap()
    }

    #[test]
    fn the_slug_filter_is_registered() {
        assert_eq!(render(r#"{{ "Hello World" | slug }}"#), "hello-world");
    }

    #[test]
    fn the_striptags_filter_is_registered() {
        assert_eq!(render(r#"{{ "<b>Joel</b>" | striptags }}"#), "Joel");
    }

    #[test]
    fn the_base64_filters_round_trip() {
        assert_eq!(render(r#"{{ "hi" | b64_encode | b64_decode }}"#), "hi");
    }
}
