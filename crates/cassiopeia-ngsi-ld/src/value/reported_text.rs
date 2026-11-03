use crate::value::types::Value;
use std::borrow::Cow;

impl Value {
    /// The text this value is quoted as when a stage reports it could not use it, or `None` when
    /// the record does not carry it.
    ///
    /// A blank value is one the record does not carry and is not reported. Text is quoted as
    /// written, and any other value (a number, a boolean, an object) as its rendering, so a mapping
    /// mistake is named rather than lost. Text is borrowed from the value: a sink copies it only when
    /// it opens a record, so a repeat of a known failure allocates nothing.
    #[must_use]
    pub fn reported_text(&self) -> Option<Cow<'_, str>> {
        match self {
            Value::String(text) => {
                let text = text.trim();
                if text.is_empty() { None } else { Some(Cow::Borrowed(text)) }
            }
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => {
                Some(Cow::Owned(self.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value::types::Value;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn a_blank_value_is_absent_rather_than_reported() {
        assert_eq!(Value::try_from(json!("   ")).unwrap().reported_text(), None);
        assert_eq!(Value::try_from(json!("")).unwrap().reported_text(), None);
    }

    #[test]
    fn text_is_reported_as_written_and_any_other_shape_as_its_rendering() {
        assert_eq!(Value::try_from(json!("  tomorrow ")).unwrap().reported_text(), Some("tomorrow".into()));
        assert_eq!(Value::try_from(json!(2026)).unwrap().reported_text(), Some("2026".into()));
        assert_eq!(Value::try_from(json!(true)).unwrap().reported_text(), Some("true".into()));
        assert_eq!(Value::try_from(json!({"en": "hi"})).unwrap().reported_text(), Some(r#"{"en":"hi"}"#.into()));
    }
}
