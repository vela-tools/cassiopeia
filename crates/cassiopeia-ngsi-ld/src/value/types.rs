use cassiopeia_geometry::geometry::NgsiLdGeometry;
use chrono::{DateTime, SecondsFormat, Utc};
use compact_str::CompactString;
use foldhash::fast::RandomState;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;

/// The contents of a structured [`Value::Object`], in the order its keys were resolved.
///
/// The keys are attribute or language-map names a mapping declared, so the map hashes with
/// `foldhash` rather than the standard library's `SipHash`.
pub type ValueObject = IndexMap<CompactString, Value, RandomState>;

/// Represents an NGSI-LD Value, optimized for performance.
/// Uses `CompactString` for inline small strings to minimize heap allocations.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Value {
    /// A JSON null value
    Null,
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

// Manual because each variant maps to a distinct JSON shape (null, bare scalar, geometry object)
// that no single derive expresses.
impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Value::Null => serializer.serialize_none(),
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Number {
    Integer(i64),
    Float(f64),
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub enum TemporalValue {
    DateTime(DateTime<Utc>),
    Date(DateTime<Utc>),
    Time(DateTime<Utc>),
}

// Manual because the wire form is a formatted RFC 3339 / date / time string, not the underlying
// `DateTime` representation.
impl Serialize for TemporalValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = match self {
            TemporalValue::DateTime(dt) => dt.to_rfc3339_opts(SecondsFormat::Millis, true),
            TemporalValue::Date(dt) => dt.format("%Y-%m-%d").to_string(),
            TemporalValue::Time(dt) => dt.format("%H:%M:%S").to_string(),
        };
        serializer.serialize_str(&s)
    }
}

impl Value {
    /// Whether the value is JSON null.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

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
            Value::Null | Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => false,
        }
    }

    /// The string contents, when the value is a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            Value::Null | Value::Boolean(_) | Value::Number(_) | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, ""),
            Value::Boolean(b) => write!(f, "{b}"),
            Value::Number(n) => match n {
                Number::Integer(i) => write!(f, "{i}"),
                Number::Float(fl) => write!(f, "{fl}"),
            },
            Value::String(s) => write!(f, "{s}"),
            Value::Temporal(t) => match t {
                TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt) => {
                    write!(f, "{}", dt.to_rfc3339_opts(SecondsFormat::Millis, true))
                }
            },
            Value::Geospatial(g) => write!(f, "{g}"),
            Value::Array(arr) => write!(f, "{arr:?}"),
            Value::Object(obj) => write!(f, "{obj:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value::types::{Number, TemporalValue, Value};
    use chrono::{TimeZone, Utc};
    use compact_str::CompactString;
    use serde_json::json;

    #[test]
    fn display_renders_each_scalar_variant() {
        assert_eq!(Value::Boolean(true).to_string(), "true");
        assert_eq!(Value::Number(Number::Integer(100)).to_string(), "100");
        assert_eq!(Value::String(CompactString::from("hello")).to_string(), "hello");
        assert_eq!(Value::Null.to_string(), "");
    }

    #[test]
    fn a_temporal_value_serialises_to_its_wire_string() {
        let dt = Utc.with_ymd_and_hms(2024, 3, 13, 12, 0, 0).unwrap();
        let value = Value::Temporal(TemporalValue::DateTime(dt));
        assert_eq!(serde_json::to_value(&value).unwrap(), json!("2024-03-13T12:00:00.000Z"));
    }

    #[test]
    fn as_str_and_is_empty_string_only_apply_to_strings() {
        assert_eq!(Value::String(CompactString::from("x")).as_str(), Some("x"));
        assert_eq!(Value::Null.as_str(), None);
        assert!(Value::String(CompactString::new("")).is_empty_string());
        assert!(!Value::Number(Number::Integer(0)).is_empty_string());
    }
}
