use serde_json::Value;
use strum::Display;

/// The shape of a JSON value that is not text, as a refused scope names it.
///
/// A scope is a string of `/`-separated levels (ETSI GS CIM 009 v1.9.1 clause 4.18), so a scope
/// template that reads a value of one of these shapes would write the value's JSON rendering, such
/// as `true`, as a scope the source never meant.
#[derive(Debug, Clone, Copy, Display, PartialEq, Eq, Hash)]
pub enum NonTextShape {
    /// A JSON boolean.
    #[strum(to_string = "a boolean")]
    Boolean,
    /// A JSON number.
    #[strum(to_string = "a number")]
    Number,
    /// A JSON array.
    #[strum(to_string = "an array")]
    Array,
    /// A JSON object.
    #[strum(to_string = "an object")]
    Object,
}

impl NonTextShape {
    /// The shape of `value`, or `None` when it is text or null: null is an absent field, which a
    /// scope template reads as no scope.
    #[must_use]
    pub const fn of(value: &Value) -> Option<NonTextShape> {
        match value {
            Value::Null | Value::String(_) => None,
            Value::Bool(_) => Some(NonTextShape::Boolean),
            Value::Number(_) => Some(NonTextShape::Number),
            Value::Array(_) => Some(NonTextShape::Array),
            Value::Object(_) => Some(NonTextShape::Object),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::urn::non_text_shape::NonTextShape;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn every_json_value_that_is_not_text_or_null_has_its_shape() {
        let shapes = [json!(true), json!(3), json!([1]), json!({"a": 1})].map(|value| NonTextShape::of(&value));

        assert_eq!(
            shapes,
            [
                Some(NonTextShape::Boolean),
                Some(NonTextShape::Number),
                Some(NonTextShape::Array),
                Some(NonTextShape::Object)
            ]
        );
    }

    #[test]
    fn text_and_null_have_no_non_text_shape() {
        assert_eq!(NonTextShape::of(&json!("/Madrid")), None);
        assert_eq!(NonTextShape::of(&json!(null)), None);
    }

    #[test]
    fn a_shape_is_named_with_its_article() {
        let names = [NonTextShape::Boolean, NonTextShape::Number, NonTextShape::Array, NonTextShape::Object].map(|shape| shape.to_string());

        assert_eq!(names, ["a boolean", "a number", "an array", "an object"]);
    }
}
