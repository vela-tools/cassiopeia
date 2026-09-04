use tera::Tera;
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
pub fn register(tera: &mut Tera) {
    tera.register_filter("b64_decode", b64_decode);
    tera.register_filter("b64_encode", b64_encode);
    tera.register_filter("date", date);
    tera.register_filter("filesize_format", filesize_format);
    tera.register_filter("format", format);
    tera.register_filter("json_encode", json_encode);
    tera.register_filter("regex_replace", RegexReplace::default());
    tera.register_filter("shuffle", shuffle);
    tera.register_filter("slug", slug);
    tera.register_filter("spaceless", spaceless);
    tera.register_filter("striptags", striptags);
    tera.register_filter("urlencode", urlencode);
    tera.register_filter("urlencode_strict", urlencode_strict);
    tera.register_function("get_random", get_random);
    tera.register_function("now", now);
    tera.register_test("after", is_after);
    tera.register_test("before", is_before);
    tera.register_test("matching", Matching::default());
}

#[cfg(test)]
mod tests {
    use crate::template::contrib::register;
    use tera::{Context, Tera};

    fn render(template: &str) -> String {
        let mut tera = Tera::default();
        register(&mut tera);
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
