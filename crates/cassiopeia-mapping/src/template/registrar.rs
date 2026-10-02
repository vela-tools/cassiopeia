use crate::template::vocabulary::{CallableKind, Vocabulary};
use tera::{ArgFromValue, Filter, Function, FunctionResult, Tera, Test, TestResult};

/// Registers filters, functions, and tests with a Tera engine and records each name it registers.
///
/// Tera can say a template names a filter it does not know but cannot list the ones it does, so the
/// names are recorded here as they are registered; that record is what a misspelt name is matched
/// against. Registering through anything but a registrar would leave the name out of it.
pub struct Registrar {
    /// The engine being populated.
    tera: Tera,
    /// The names registered with it so far, Tera's built-ins included.
    vocabulary: Vocabulary,
}

impl Registrar {
    /// A registrar over an engine holding only Tera's built-ins.
    #[must_use]
    pub fn with_engine_builtins() -> Registrar {
        Registrar {
            tera: Tera::default(),
            vocabulary: Vocabulary::of_engine_builtins(),
        }
    }

    /// Registers `filter` under `name`, replacing any filter already registered under it.
    pub fn filter<Func, Arg, Res>(&mut self, name: &'static str, filter: Func)
    where
        Func: Filter<Arg, Res> + for<'a> Filter<<Arg as ArgFromValue<'a>>::Output, Res>,
        Arg: for<'a> ArgFromValue<'a>,
        Res: FunctionResult,
    {
        self.tera.register_filter(name, filter);
        self.vocabulary.record(CallableKind::Filter, name);
    }

    /// Registers `test` under `name`, replacing any test already registered under it.
    pub fn test<Func, Arg, Res>(&mut self, name: &'static str, test: Func)
    where
        Func: Test<Arg, Res> + for<'a> Test<<Arg as ArgFromValue<'a>>::Output, Res>,
        Arg: for<'a> ArgFromValue<'a>,
        Res: TestResult,
    {
        self.tera.register_test(name, test);
        self.vocabulary.record(CallableKind::Test, name);
    }

    /// Registers `function` under `name`, replacing any function already registered under it.
    pub fn function<Func, Res>(&mut self, name: &'static str, function: Func)
    where
        Func: Function<Res>,
        Res: FunctionResult,
    {
        self.tera.register_function(name, function);
        self.vocabulary.record(CallableKind::Function, name);
    }

    /// The populated engine and the names registered with it.
    #[must_use]
    pub fn finish(self) -> (Tera, Vocabulary) {
        (self.tera, self.vocabulary)
    }
}

#[cfg(test)]
mod tests {
    use crate::template::{registrar::Registrar, vocabulary::CallableKind};
    use tera::{Context, Kwargs, State};

    #[test]
    fn a_registered_filter_renders_and_its_name_is_recorded() {
        let mut registrar = Registrar::with_engine_builtins();
        registrar.filter("double", |value: i64, _: Kwargs, _: &State| value * 2);
        let (tera, vocabulary) = registrar.finish();

        assert_eq!(tera.render_str("{{ 21 | double }}", &Context::new(), false).unwrap(), "42");
        assert_eq!(vocabulary.closest(CallableKind::Filter, "doubel"), Some("double"));
    }

    #[test]
    fn a_registered_function_and_test_are_recorded_under_their_kinds() {
        let mut registrar = Registrar::with_engine_builtins();
        registrar.function("answer", |_: Kwargs, _: &State| 42);
        registrar.test("positive", |value: i64, _: Kwargs, _: &State| value > 0);
        let (tera, vocabulary) = registrar.finish();

        assert_eq!(
            tera.render_str("{% if answer() is positive %}yes{% endif %}", &Context::new(), false).unwrap(),
            "yes"
        );
        assert_eq!(vocabulary.closest(CallableKind::Function, "answr"), Some("answer"));
        assert_eq!(vocabulary.closest(CallableKind::Test, "positiv"), Some("positive"));
        assert_eq!(vocabulary.closest(CallableKind::Filter, "answr"), None);
    }
}
