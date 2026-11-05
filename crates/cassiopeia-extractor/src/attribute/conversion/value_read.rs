use cassiopeia_ngsi_ld::value::types::Value;
use serde_json::Value as JsonValue;

/// What one reading of a declaration's source produced, before the declaration's type admits it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ReadValue {
    /// Raw JSON the `object` or `json` reading kept as the source held it, every null included.
    Json(JsonValue),
    /// A value a scalar, text, temporal, identifier, or geometry reading built.
    Value(Value),
}

/// Writes a reading as the JSON it stands for inside a structured value, such as a member of an
/// object nested `mappings` assemble: raw JSON as it was kept, nulls included, and a value as the
/// JSON it serializes to.
// Manual because each variant unwraps into the JSON it holds or stands for, which no derive
// expresses.
impl From<ReadValue> for JsonValue {
    fn from(reading: ReadValue) -> JsonValue {
        match reading {
            ReadValue::Json(json) => json,
            ReadValue::Value(value) => JsonValue::from(value),
        }
    }
}

/// What reading one value found: what the reading built, or why the value carried nothing to read.
///
/// Where the value stands decides what each absence means: a `Property` holds no value for any of
/// them, while a `ListProperty` leaves out a null item and refuses an item that is there but reads
/// as nothing, since no item may vanish from a list unannounced.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ValueRead {
    /// What the reading built.
    Read(ReadValue),
    /// Why the value carried nothing to read.
    Absent(Absence),
}

impl ValueRead {
    /// What the reading built, or `None` whichever way the value carried nothing.
    pub(crate) fn into_read(self) -> Option<ReadValue> {
        match self {
            ValueRead::Read(read) => Some(read),
            ValueRead::Absent(Absence::Null | Absence::Blank(_) | Absence::SpelledNull) => None,
        }
    }
}

/// Why a value carried nothing to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Absence {
    /// The value is null, a field the record does not carry.
    Null,
    /// The value is blank text (see [`is_blank`](cassiopeia_ngsi_ld::value::reported_text::is_blank)),
    /// which every reading but text reads as a field the record does not carry, kept as written so
    /// a refusal can quote it.
    Blank(String),
    /// The value is JSON text spelling `null`, which the `json` reading reads as the null it spells.
    SpelledNull,
}

#[cfg(test)]
mod tests {
    use crate::attribute::conversion::value_read::{Absence, ReadValue, ValueRead};
    use cassiopeia_ngsi_ld::value::types::Value;
    use pretty_assertions::assert_eq;
    use serde_json::{Value as JsonValue, json};

    #[test]
    fn a_read_value_is_what_the_reading_built() {
        let read = ReadValue::Value(Value::Boolean(true));

        assert_eq!(ValueRead::Read(read.clone()).into_read(), Some(read));
    }

    #[test]
    fn every_absence_reads_as_no_value() {
        for absence in [Absence::Null, Absence::Blank("  ".to_owned()), Absence::SpelledNull] {
            assert_eq!(ValueRead::Absent(absence.clone()).into_read(), None, "{absence:?}");
        }
    }

    #[test]
    fn a_reading_is_written_as_the_json_it_stands_for() {
        assert_eq!(JsonValue::from(ReadValue::Json(json!({"a": null}))), json!({"a": null}));
        assert_eq!(JsonValue::from(ReadValue::Value(Value::Boolean(true))), json!(true));
    }
}
