use tera::{Kwargs, State, TeraResult, Value, value::Key};

/// Tera filter `get`: reads the entry named by `key` from a map value, returning the value found,
/// the `default` when one is supplied, or an empty result when the key is absent.
///
/// This replaces Tera's built-in `get`, which raises an error for a missing key when no default is
/// set. Reading an absent field is normal for sparse records (a translation a dataset simply does
/// not carry, say), and is handled downstream by skipping the empty value, exactly as a missing
/// `{{ field }}` lookup resolves to nothing. Erroring instead would fail a whole entity over one
/// absent optional value.
///
/// The key is matched literally, never as a path. That is the whole reason the filter form exists:
/// a key is data, and real datasets use keys that a dotted path cannot express: `pt-BR`, an OBIS
/// register code such as `1-0:1.8.0`, a nutrient name such as `energy-kcal_100g`. Splitting on `.`
/// would silently miss every key containing one and report the entry as absent.
///
/// # Errors
/// Returns a [`tera::Error`] when the mandatory `key` argument is absent or is not a string.
// `Kwargs` is taken by value because tera's blanket `Filter` impl is `Fn(Arg, Kwargs, &State)`;
// borrowing it would no longer satisfy the trait and `register_filter` would fail to compile.
#[allow(clippy::needless_pass_by_value, reason = "tera's Filter trait requires Kwargs by value")]
pub fn get(value: &Value, kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let key = kwargs.must_get::<&str>("key")?;
    let default = kwargs.get::<Value>("default")?;

    if let Some(found) = value.as_map().and_then(|map| map.get(&Key::Str(key)))
        && !found.is_undefined()
    {
        return Ok(found.clone());
    }

    Ok(default.unwrap_or_else(Value::none))
}

#[cfg(test)]
mod tests {
    use crate::template::filter::get::get;
    use std::collections::BTreeMap;
    use tera::{Context, Tera};

    fn render(template: &str, key: &str, translations: &[(&str, &str)]) -> String {
        let mut tera = Tera::default();
        tera.register_filter("get", get);
        tera.add_raw_template("t", template).unwrap();

        let mut context = Context::new();
        let map: BTreeMap<&str, &str> = translations.iter().copied().collect();
        context.insert("translations", &map);
        context.insert("key", key);

        tera.render("t", &context).unwrap()
    }

    #[test]
    fn a_present_key_returns_its_value() {
        assert_eq!(render(r#"{{ translations | get(key="pt-BR") }}"#, "pt-BR", &[("pt-BR", "Brasil")]), "Brasil");
    }

    #[test]
    fn a_missing_key_renders_empty_rather_than_erroring() {
        assert_eq!(render(r#"{{ translations | get(key="pt-BR") }}"#, "pt-BR", &[("de", "Brasilien")]), "");
    }

    #[test]
    fn a_missing_key_falls_back_to_a_supplied_default() {
        assert_eq!(
            render(r#"{{ translations | get(key="pt-BR", default="none") }}"#, "pt-BR", &[("de", "Brasilien")]),
            "none"
        );
    }

    #[test]
    fn a_key_containing_dots_is_matched_literally_rather_than_walked_as_a_path() {
        // An OBIS register code is a single map key that happens to contain dots. Treating it as a
        // dotted path splits it into segments that match nothing, so every such lookup reports the
        // entry as absent and, chained into a filter expecting an array, fails the whole entity.
        assert_eq!(
            render(r#"{{ translations | get(key="1-0:1.8.0") }}"#, "1-0:1.8.0", &[("1-0:1.8.0", "19332.701")]),
            "19332.701"
        );
    }

    #[test]
    fn a_dotted_key_does_not_traverse_into_a_nested_map() {
        // The counterpart of the rule above: `a.b` names one key, so it must not reach a nested `b`.
        let mut tera = Tera::default();
        tera.register_filter("get", get);
        tera.add_raw_template("t", r#"{{ outer | get(key="a.b") }}"#).unwrap();

        let mut context = Context::new();
        let mut nested = BTreeMap::new();
        nested.insert("b", "nested");
        let mut outer: BTreeMap<&str, BTreeMap<&str, &str>> = BTreeMap::new();
        outer.insert("a", nested);
        context.insert("outer", &outer);

        assert_eq!(tera.render("t", &context).unwrap(), "");
    }

    #[test]
    fn a_non_map_value_yields_nothing_rather_than_erroring() {
        let mut tera = Tera::default();
        tera.register_filter("get", get);
        tera.add_raw_template("t", r#"{{ scalar | get(key="anything") }}"#).unwrap();

        let mut context = Context::new();
        context.insert("scalar", &7);

        assert_eq!(tera.render("t", &context).unwrap(), "");
    }
}
