use strum::{Display, EnumString};

/// The three kinds of named callable a template can refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum CallableKind {
    /// A filter, applied as `value | name`.
    Filter,
    /// A function, called as `name()`.
    Function,
    /// A test, applied as `value is name`.
    Test,
}

/// The filters `Tera::default()` registers in Tera 2.4.
///
/// Tera offers no way to list what an engine has registered, so its built-ins are named here. Each is
/// checked against the engine by a test; one Tera adds later is merely never suggested.
const ENGINE_FILTERS: [&str; 37] = [
    "safe",
    "default",
    "upper",
    "lower",
    "wordcount",
    "escape",
    "escape_html",
    "escape_xml",
    "newlines_to_br",
    "pluralize",
    "trim",
    "trim_start",
    "trim_end",
    "replace",
    "capitalize",
    "title",
    "truncate",
    "indent",
    "str",
    "int",
    "float",
    "length",
    "reverse",
    "split",
    "abs",
    "round",
    "first",
    "last",
    "nth",
    "join",
    "sort",
    "unique",
    "get",
    "values",
    "keys",
    "pairs",
    "group_by",
];

/// The tests `Tera::default()` registers in Tera 2.4.
const ENGINE_TESTS: [&str; 17] = [
    "string",
    "number",
    "map",
    "bool",
    "array",
    "integer",
    "float",
    "none",
    "iterable",
    "defined",
    "undefined",
    "odd",
    "even",
    "divisible_by",
    "starting_with",
    "ending_with",
    "containing",
];

/// The functions `Tera::default()` registers in Tera 2.4.
const ENGINE_FUNCTIONS: [&str; 2] = ["range", "throw"];

/// The names of every filter, function, and test registered with the template engine, kept to
/// suggest the intended name when a template refers to one that is not registered.
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    /// Each registered name with its kind; a name registered twice, as Cassiopeia's `get` replaces
    /// Tera's, appears twice, which changes no suggestion.
    names: Vec<(CallableKind, &'static str)>,
}

impl Vocabulary {
    /// The vocabulary of an engine that has only Tera's built-ins registered.
    #[must_use]
    pub fn of_engine_builtins() -> Vocabulary {
        let filters = ENGINE_FILTERS.iter().map(|&name| (CallableKind::Filter, name));
        let tests = ENGINE_TESTS.iter().map(|&name| (CallableKind::Test, name));
        let functions = ENGINE_FUNCTIONS.iter().map(|&name| (CallableKind::Function, name));

        Vocabulary {
            names: filters.chain(tests).chain(functions).collect(),
        }
    }

    /// Records one more registered name.
    pub fn record(&mut self, kind: CallableKind, name: &'static str) {
        self.names.push((kind, name));
    }

    /// The registered `kind` name closest to the unregistered `name`, when one is close enough to be
    /// a likely misspelling of it.
    ///
    /// Closeness is the Damerau-Levenshtein distance, so a transposition (`lenght`) costs one edit;
    /// a name qualifies within one edit per three characters, and at least one.
    #[must_use]
    pub fn closest(&self, kind: CallableKind, name: &str) -> Option<&'static str> {
        let tolerance = (name.chars().count() / 3).max(1);

        self.names
            .iter()
            .filter(|&&(registered_kind, _)| registered_kind == kind)
            .map(|&(_, registered)| (strsim::damerau_levenshtein(name, registered), registered))
            .filter(|&(distance, _)| distance <= tolerance)
            .min()
            .map(|(_, registered)| registered)
    }
}

#[cfg(test)]
mod tests {
    use crate::template::vocabulary::{CallableKind, ENGINE_FILTERS, ENGINE_FUNCTIONS, ENGINE_TESTS, Vocabulary};
    use tera::Tera;

    #[test]
    fn every_listed_engine_builtin_is_registered_by_the_engine() {
        let mut tera = Tera::default();
        for filter in ENGINE_FILTERS {
            assert!(tera.add_raw_template("probe", &format!("{{{{ a | {filter} }}}}")).is_ok(), "filter {filter}");
        }
        for test in ENGINE_TESTS {
            assert!(
                tera.add_raw_template("probe", &format!("{{% if a is {test} %}}x{{% endif %}}")).is_ok(),
                "test {test}"
            );
        }
        for function in ENGINE_FUNCTIONS {
            assert!(
                tera.add_raw_template("probe", &format!("{{{{ {function}() }}}}")).is_ok(),
                "function {function}"
            );
        }
    }

    #[test]
    fn a_misspelling_suggests_the_registered_name_of_the_same_kind() {
        let vocabulary = Vocabulary::of_engine_builtins();

        assert_eq!(vocabulary.closest(CallableKind::Filter, "uper"), Some("upper"));
        assert_eq!(vocabulary.closest(CallableKind::Filter, "lenght"), Some("length"));
        assert_eq!(vocabulary.closest(CallableKind::Test, "evn"), Some("even"));
        assert_eq!(vocabulary.closest(CallableKind::Function, "rang"), Some("range"));
    }

    #[test]
    fn a_name_far_from_every_registered_one_has_no_suggestion() {
        let vocabulary = Vocabulary::of_engine_builtins();

        assert_eq!(vocabulary.closest(CallableKind::Filter, "no_such_filter"), None);
        assert_eq!(vocabulary.closest(CallableKind::Function, "upper"), None);
    }

    #[test]
    fn a_recorded_name_can_be_suggested() {
        let mut vocabulary = Vocabulary::of_engine_builtins();
        vocabulary.record(CallableKind::Filter, "json_decode");

        assert_eq!(vocabulary.closest(CallableKind::Filter, "json_decod"), Some("json_decode"));
    }

    #[test]
    fn a_kind_parses_from_and_renders_as_teras_word_for_it() {
        assert_eq!("filter".parse::<CallableKind>().ok(), Some(CallableKind::Filter));
        assert_eq!(CallableKind::Function.to_string(), "function");
    }
}
