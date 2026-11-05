use crate::{attribute::refusal::AttributeRefusal, required_value::RequiredValue};
use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};
use serde_json::Value as JsonValue;

/// What admitting raw JSON as the NGSI-LD Value a `Property` or a `ListProperty` holds came to.
///
/// No null can sit anywhere in a Value (ETSI GS CIM 009 v1.9.1 clause 4.5.0), so admitting leaves
/// out every null nested in the JSON. What is left over decides the outcome, and the place the
/// value stands decides what each outcome means (see [`Admission::into_value`] and
/// [`Admission::into_item`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Admission {
    /// The value, without its nulls.
    Admitted(Value),
    /// An object or array with nothing in it.
    Empty(JsonValue),
    /// An object or array that held something but nothing except nulls, or a null itself: writing
    /// it without its nulls would leave nothing.
    OnlyNulls(JsonValue),
}

impl Admission {
    /// What the admission makes of a whole value.
    ///
    /// An empty object or array is a value the record does not carry, as an absent source is.
    ///
    /// # Errors
    /// Returns the refusal of a value of only nulls (see [`only_nulls_refusal`]): the source carried
    /// it, so the run names it rather than losing it unannounced.
    pub(crate) fn into_value(self) -> Result<Option<Value>, AttributeRefusal> {
        match self {
            Admission::Admitted(value) => Ok(Some(value)),
            Admission::Empty(_) => Ok(None),
            Admission::OnlyNulls(json) => Err(only_nulls_refusal(&json)),
        }
    }

    /// What the admission makes of one item of a list.
    ///
    /// The list was there, so an item that admits nothing is refused rather than left out: a list's
    /// order carries meaning and no item may vanish from it unannounced.
    ///
    /// # Errors
    /// Returns the refusal of an empty item or one of only nulls, quoting it, as not
    /// [`RequiredValue::NonNull`].
    pub(crate) fn into_item(self) -> Result<Value, AttributeRefusal> {
        match self {
            Admission::Admitted(value) => Ok(value),
            Admission::Empty(json) | Admission::OnlyNulls(json) => Err(only_nulls_refusal(&json)),
        }
    }
}

/// Admits raw JSON as the NGSI-LD Value a `Property` or a `ListProperty` holds, leaving out every
/// null nested in it and counting each in `omitted`.
///
/// Nothing is counted for JSON that admits nothing: no value is written without its nulls.
pub(crate) fn admit(json: JsonValue, omitted: &mut OmittedNulls) -> Admission {
    let empty = match &json {
        JsonValue::Null => return Admission::OnlyNulls(json),
        JsonValue::Array(items) if only_nulls(items.iter(), JsonValue::is_null) => return Admission::OnlyNulls(json),
        JsonValue::Object(members) if only_nulls(members.values(), JsonValue::is_null) => return Admission::OnlyNulls(json),
        JsonValue::Array(items) => items.is_empty(),
        JsonValue::Object(members) => members.is_empty(),
        JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => false,
    };
    if empty {
        return Admission::Empty(json);
    }

    // Past the checks above, the JSON holds something other than null, so a value is left.
    Value::from_json(json, omitted).map_or(Admission::OnlyNulls(JsonValue::Null), Admission::Admitted)
}

/// Whether a container's entries, at least one of them, are every one null.
pub(crate) fn only_nulls<I: ExactSizeIterator>(mut entries: I, is_null: impl FnMut(I::Item) -> bool) -> bool {
    entries.len() > 0 && entries.all(is_null)
}

/// The refusal of a value that holds nothing but nulls, or nothing at all, quoting it as its
/// compact JSON text, as not [`RequiredValue::NonNull`].
pub(crate) fn only_nulls_refusal(json: &JsonValue) -> AttributeRefusal {
    AttributeRefusal::unusable(RequiredValue::NonNull, &json.to_string())
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{
            conversion::value_admission::{Admission, admit, only_nulls},
            refusal::AttributeRefusal,
        },
        required_value::RequiredValue,
    };
    use cassiopeia_ngsi_ld::value::{omitted_nulls::OmittedNulls, types::Value};
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    fn admitted(json: JsonValue) -> (Result<Option<Value>, AttributeRefusal>, OmittedNulls) {
        let mut omitted = OmittedNulls::default();
        let admitted = admit(json, &mut omitted).into_value();
        (admitted, omitted)
    }

    #[test]
    fn json_is_admitted_without_its_nulls_and_counts_them() {
        let (value, omitted) = admitted(json!({"value": 21.5, "unit": null, "tags": ["a", null]}));

        assert_eq!(value, Ok(Some(Value::try_from(json!({"value": 21.5, "tags": ["a"]})).unwrap())));
        assert_eq!(
            omitted,
            OmittedNulls {
                array_items: 1,
                object_members: 1,
            }
        );
    }

    #[test]
    fn an_object_or_array_of_only_nulls_is_refused_quoting_it_and_counts_nothing() {
        for (json, quoted) in [(json!({"maybe": null}), r#"{"maybe":null}"#), (json!([null, null]), "[null,null]")] {
            let (value, omitted) = admitted(json);

            assert_eq!(value, Err(AttributeRefusal::unusable(RequiredValue::NonNull, quoted)));
            assert!(omitted.is_empty(), "{quoted}");
        }
    }

    #[test]
    fn an_empty_object_or_array_is_absent() {
        for json in [json!({}), json!([])] {
            assert_eq!(admitted(json).0, Ok(None));
        }
    }

    #[test]
    fn an_object_holding_an_object_of_only_nulls_keeps_the_emptied_member() {
        let (value, omitted) = admitted(json!({"a": {"b": null}}));

        assert_eq!(value, Ok(Some(Value::try_from(json!({"a": {}})).unwrap())));
        assert_eq!(omitted.object_members, 1);
    }

    #[test]
    fn admitting_tells_an_empty_container_from_one_of_only_nulls() {
        for (json, admission) in [
            (json!({}), Admission::Empty(json!({}))),
            (json!([]), Admission::Empty(json!([]))),
            (json!([null]), Admission::OnlyNulls(json!([null]))),
            (json!(null), Admission::OnlyNulls(json!(null))),
            (json!([1, null]), Admission::Admitted(Value::try_from(json!([1])).unwrap())),
        ] {
            assert_eq!(admit(json, &mut OmittedNulls::default()), admission);
        }
    }

    #[test]
    fn an_item_that_admits_nothing_is_refused_quoting_it() {
        for (admission, quoted) in [(Admission::Empty(json!({})), "{}"), (Admission::OnlyNulls(json!([null])), "[null]")] {
            assert_eq!(admission.into_item(), Err(AttributeRefusal::unusable(RequiredValue::NonNull, quoted)));
        }
        assert_eq!(Admission::Admitted(Value::Boolean(true)).into_item(), Ok(Value::Boolean(true)));
    }

    #[test]
    fn only_entries_that_are_every_one_null_and_at_least_one_hold_only_nulls() {
        assert!(only_nulls([JsonValue::Null].iter(), JsonValue::is_null));
        assert!(!only_nulls(Vec::<JsonValue>::new().iter(), JsonValue::is_null));
        assert!(!only_nulls([JsonValue::Null, json!(1)].iter(), JsonValue::is_null));
    }
}
