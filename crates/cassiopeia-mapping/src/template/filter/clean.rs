use tera::{Kwargs, State};

/// Tera filter `clean`: folds every whitespace character to an ASCII space, collapses runs of
/// whitespace into one space, and trims the ends.
///
/// Open datasets routinely carry non-breaking spaces and stray newlines inside label fields;
/// entity names built from them would otherwise differ only by invisible characters.
///
/// A non-string value is rejected by Tera before this function runs: the `&str` argument makes the
/// engine cast the piped value and raise its own error when the cast fails.
#[must_use]
pub fn clean(value: &str, _kwargs: Kwargs, _state: &State) -> String {
    let normalized: String = value.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();

    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use crate::template::filter::clean::clean;
    use tera::{Context, Tera};

    fn apply(input: &str) -> String {
        let mut tera = Tera::default();
        tera.register_filter("clean", clean);
        tera.add_raw_template("t", "{{ value | clean }}").unwrap();

        let mut context = Context::new();
        context.insert("value", input);

        tera.render("t", &context).unwrap()
    }

    #[test]
    fn collapses_runs_of_spaces() {
        assert_eq!(apply("Main   Street"), "Main Street");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(apply("  Main Street  "), "Main Street");
    }

    #[test]
    fn folds_non_ascii_whitespace_to_a_plain_space() {
        assert_eq!(apply("Main\u{a0}Street"), "Main Street");
    }

    #[test]
    fn replaces_newlines_and_tabs() {
        assert_eq!(apply("Main\n\tStreet"), "Main Street");
    }

    #[test]
    fn a_whitespace_only_value_becomes_empty() {
        assert_eq!(apply("   \n "), "");
    }

    #[test]
    fn a_non_string_value_is_rejected() {
        let mut tera = Tera::default();
        tera.register_filter("clean", clean);
        tera.add_raw_template("t", "{{ value | clean }}").unwrap();

        let mut context = Context::new();
        context.insert("value", &true);

        assert!(tera.render("t", &context).is_err());
    }
}
