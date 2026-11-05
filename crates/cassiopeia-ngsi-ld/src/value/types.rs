use crate::value::finite_float::FiniteFloat;
use cassiopeia_geometry::geometry::NgsiLdGeometry;
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use foldhash::fast::RandomState;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;
use strum::EnumDiscriminants;

/// The contents of a structured [`Value::Object`], in the order its keys were resolved.
///
/// The keys are attribute or language-map names a mapping declared, so the map hashes with
/// `foldhash` rather than the standard library's `SipHash`.
pub type ValueObject = IndexMap<CompactString, Value, RandomState>;

/// Represents an NGSI-LD Value, optimized for performance.
/// Uses `CompactString` for inline small strings to minimize heap allocations.
///
/// There is no null: an NGSI-LD Value is a string, a number, a boolean, an object or an array (ETSI
/// GS CIM 009 v1.9.1 clause 3.1), and `null` cannot be used as a value anywhere in an entity (clause
/// 4.5.0), so neither the value itself nor any member or item nested in it can be one. A value that
/// does not exist is not built: it is the `None` of an `Option<Value>`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Value {
    /// A boolean value
    Boolean(bool),
    /// A numeric value (internally either i64 or f64)
    Number(Number),
    /// A string value (optimized for small strings)
    String(CompactString),
    /// A temporal value (`DateTime`, Date, or Time)
    Temporal(TemporalValue),
    /// A geospatial value: one of the six geometry types a `GeoProperty` admits (ETSI GS CIM 009
    /// v1.9.1, clause 4.7), boxed to keep the enum size small.
    Geospatial(Box<NgsiLdGeometry>),
    /// An array of values
    Array(Vec<Value>),
    /// A structured object (boxed to keep enum size small)
    Object(Box<ValueObject>),
}

// Manual because each variant maps to a distinct JSON shape (bare scalar, time text, geometry
// object) that no single derive expresses.
impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Value::Boolean(b) => serializer.serialize_bool(*b),
            Value::Number(n) => n.serialize(serializer),
            Value::String(s) => s.serialize(serializer),
            Value::Temporal(t) => t.serialize(serializer),
            Value::Geospatial(g) => g.serialize(serializer),
            Value::Array(arr) => arr.serialize(serializer),
            Value::Object(obj) => obj.serialize(serializer),
        }
    }
}

/// An NGSI-LD number: an integer when it fits `i64`, otherwise a float.
///
/// The float is always finite, because JSON has no number for any other (RFC 8259 clause 6).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Number {
    /// A whole number that fits `i64`.
    Integer(i64),
    /// A finite float.
    Float(FiniteFloat),
}

/// A value of one of the three time value data types of ETSI GS CIM 009 v1.9.1 clause 4.6.3.
///
/// Its variant names the data type, which [`TemporalValueType`] carries into a Property's
/// `valueType` member so a broker reads the value as a time rather than as a string.
#[derive(Debug, Clone, Deserialize, PartialEq, EnumDiscriminants)]
#[strum_discriminants(name(TemporalValueType), derive(Serialize, Hash))]
#[strum_discriminants(
    doc = "The data type of a [`TemporalValue`], serialized as the term clause 4.6.3 names it by (`DateTime`, `Date` or `Time`), which the NGSI-LD core context maps to its datatype URI."
)]
pub enum TemporalValue {
    /// A calendar date with a time of day, in UTC.
    DateTime(DateTime<Utc>),
    /// The calendar date of the instant.
    Date(DateTime<Utc>),
    /// The time of day of the instant, in UTC.
    Time(DateTime<Utc>),
}

// Manual because the wire form is the clause 4.6.3 text of the data type, not the underlying
// `DateTime` representation.
impl Serialize for TemporalValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&self.text())
    }
}

impl Value {
    /// Whether the value is a string.
    #[must_use]
    pub const fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }

    /// Whether the value is an empty string.
    #[must_use]
    pub fn is_empty_string(&self) -> bool {
        match self {
            Value::String(s) => s.is_empty(),
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => false,
        }
    }

    /// The time value data type of the value, when it holds a time value.
    #[must_use]
    pub fn temporal_type(&self) -> Option<TemporalValueType> {
        match self {
            Value::Temporal(temporal) => Some(TemporalValueType::from(temporal)),
            Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => None,
        }
    }

    /// The string contents, when the value is a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Boolean(b) => write!(f, "{b}"),
            Value::Number(n) => match n {
                Number::Integer(i) => write!(f, "{i}"),
                Number::Float(fl) => write!(f, "{fl}"),
            },
            Value::String(s) => write!(f, "{s}"),
            Value::Temporal(t) => write!(f, "{}", t.text()),
            Value::Geospatial(g) => write!(f, "{g}"),
            // A structured value's text is its compact JSON encoding through the `Serialize` impl
            // above, the same form template resolution writes for an array or object. Encoding can
            // only fail inside a custom `Serialize`, and `fmt::Error` carries no payload to keep.
            Value::Array(_) | Value::Object(_) => f.write_str(&serde_json::to_string(self).map_err(|_| fmt::Error)?),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value::types::{Number, TemporalValue, TemporalValueType, Value};
    use chrono::{TimeZone, Utc};
    use compact_str::CompactString;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn display_renders_each_scalar_variant() {
        assert_eq!(Value::Boolean(true).to_string(), "true");
        assert_eq!(Value::Number(Number::Integer(100)).to_string(), "100");
        assert_eq!(Value::String(CompactString::from("hello")).to_string(), "hello");
    }

    #[test]
    fn display_renders_an_array_as_compact_json() {
        let value = Value::try_from(json!(["BS", "IN", 7, true])).unwrap();

        assert_eq!(value.to_string(), r#"["BS","IN",7,true]"#);
    }

    #[test]
    fn display_renders_a_nested_object_as_compact_json() {
        let value = Value::try_from(json!({"a": 1, "b": {"c": ["x", {"d": false}]}})).unwrap();

        assert_eq!(value.to_string(), r#"{"a":1,"b":{"c":["x",{"d":false}]}}"#);
    }

    #[test]
    fn display_renders_a_float_in_its_shortest_form() {
        assert_eq!(Value::try_from(3.0).unwrap().to_string(), "3");
        assert_eq!(Value::try_from(2.5).unwrap().to_string(), "2.5");
    }

    #[test]
    fn a_temporal_value_serialises_to_its_wire_string() {
        let dt = Utc.with_ymd_and_hms(2024, 3, 13, 12, 0, 0).unwrap();
        let value = Value::Temporal(TemporalValue::DateTime(dt));
        assert_eq!(serde_json::to_value(&value).unwrap(), json!("2024-03-13T12:00:00Z"));
    }

    #[test]
    fn a_temporal_value_names_its_data_type_and_a_string_names_none() {
        let dt = Utc.with_ymd_and_hms(2024, 3, 13, 12, 0, 0).unwrap();

        assert_eq!(Value::Temporal(TemporalValue::DateTime(dt)).temporal_type(), Some(TemporalValueType::DateTime));
        assert_eq!(Value::Temporal(TemporalValue::Date(dt)).temporal_type(), Some(TemporalValueType::Date));
        assert_eq!(Value::Temporal(TemporalValue::Time(dt)).temporal_type(), Some(TemporalValueType::Time));
        assert_eq!(Value::String(CompactString::from("2024-03-13T12:00:00Z")).temporal_type(), None);
    }

    #[test]
    fn a_temporal_value_type_serializes_to_the_clause_4_6_3_term() {
        assert_eq!(serde_json::to_value(TemporalValueType::DateTime).unwrap(), json!("DateTime"));
        assert_eq!(serde_json::to_value(TemporalValueType::Date).unwrap(), json!("Date"));
        assert_eq!(serde_json::to_value(TemporalValueType::Time).unwrap(), json!("Time"));
    }

    #[test]
    fn a_temporal_value_displays_as_its_wire_text() {
        let dt = Utc.with_ymd_and_hms(2024, 3, 13, 12, 0, 0).unwrap();

        assert_eq!(Value::Temporal(TemporalValue::DateTime(dt)).to_string(), "2024-03-13T12:00:00Z");
        assert_eq!(Value::Temporal(TemporalValue::Date(dt)).to_string(), "2024-03-13");
        assert_eq!(Value::Temporal(TemporalValue::Time(dt)).to_string(), "12:00:00Z");
    }

    #[test]
    fn as_str_and_is_empty_string_only_apply_to_strings() {
        assert_eq!(Value::String(CompactString::from("x")).as_str(), Some("x"));
        assert_eq!(Value::Boolean(true).as_str(), None);
        assert!(Value::String(CompactString::new("")).is_empty_string());
        assert!(!Value::Number(Number::Integer(0)).is_empty_string());
    }
}
