use crate::value::types::{Number, TemporalValue, Value};
use compact_str::CompactString;
use num_traits::ToPrimitive;
use std::str::from_utf8;

/// The longest input `parse_decimal` rewrites in a stack buffer; longer inputs fall back to a heap
/// allocation. Any real numeric literal is far shorter than this.
const DECIMAL_STACK_BYTES: usize = 32;

/// Parses a boolean from a "dirty" string, returning `None` when no boolean token is recognised.
///
/// Recognises the common textual and numeric truth tokens; anything else is not a boolean. The token
/// comparison is case-insensitive without allocating a lowercased copy of the input.
#[must_use]
pub fn parse_boolean(s: &str) -> Option<bool> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("true") || s == "1" || s.eq_ignore_ascii_case("yes") || s.eq_ignore_ascii_case("on") {
        Some(true)
    } else if s.eq_ignore_ascii_case("false") || s == "0" || s.eq_ignore_ascii_case("no") || s.eq_ignore_ascii_case("off") {
        Some(false)
    } else {
        None
    }
}

/// Parses an `i64` from a string with heuristic cleaning (surrounding quotes, comma decimals).
#[must_use]
pub fn parse_integer(s: &str) -> Option<i64> {
    let cleaned = strip_quotes(s.trim());
    if let Ok(integer) = cleaned.parse::<i64>() {
        return Some(integer);
    }
    parse_decimal(cleaned).and_then(|f| f.to_i64())
}

/// Parses an `f64` from a string with heuristic cleaning (surrounding quotes, comma decimals).
#[must_use]
pub fn parse_float(s: &str) -> Option<f64> {
    parse_decimal(strip_quotes(s.trim()))
}

/// Parses an `f64`, accepting a comma as the decimal separator, without allocating on the common
/// dot-decimal path. A comma-decimal input is rewritten in a stack buffer rather than a heap
/// `String`, so per-field parsing on ingest touches the allocator only for pathologically long input.
#[must_use]
pub fn parse_decimal(s: &str) -> Option<f64> {
    if let Ok(float) = s.parse::<f64>() {
        return Some(float);
    }
    if !s.as_bytes().contains(&b',') {
        return None;
    }

    if s.len() <= DECIMAL_STACK_BYTES {
        let mut buffer = [0u8; DECIMAL_STACK_BYTES];
        for (dst, &byte) in buffer.iter_mut().zip(s.as_bytes()) {
            *dst = if byte == b',' { b'.' } else { byte };
        }
        // Only the ASCII ',' bytes were rewritten to ASCII '.', so the remaining bytes keep their
        // original UTF-8 encoding and the slice stays valid UTF-8.
        from_utf8(&buffer[..s.len()]).ok()?.parse::<f64>().ok()
    } else {
        s.replace(',', ".").parse::<f64>().ok()
    }
}

/// Strips a single pair of matching surrounding quotes, if present.
fn strip_quotes(s: &str) -> &str {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        if s.len() >= 2 { s[1..s.len() - 1].trim() } else { s }
    } else {
        s
    }
}

/// Whether a float is non-zero, avoiding a direct floating-point equality comparison.
fn is_nonzero(f: f64) -> bool {
    f.abs() > 0.0
}

impl Value {
    /// Converts a string that might be structured JSON (array or object) into a [`Value`].
    #[must_use]
    pub fn from_string(s: String) -> Value {
        let trimmed = s.trim();
        let looks_structured = (trimmed.starts_with('[') && trimmed.ends_with(']')) || (trimmed.starts_with('{') && trimmed.ends_with('}'));
        if looks_structured && let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return Value::from(json);
        }
        Value::String(CompactString::from(s))
    }

    /// Interprets the value as a boolean, returning `None` when it carries no boolean meaning.
    #[must_use]
    pub fn to_boolean(&self) -> Option<bool> {
        match self {
            Value::Boolean(b) => Some(*b),
            Value::String(s) => parse_boolean(s),
            Value::Number(Number::Integer(i)) => Some(*i != 0),
            Value::Number(Number::Float(f)) => Some(is_nonzero(*f)),
            Value::Null | Value::Temporal(_) | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => None,
        }
    }

    /// Interprets the value as an integer, returning `None` when it cannot be represented as one.
    #[must_use]
    pub fn to_integer(&self) -> Option<i64> {
        match self {
            Value::Number(Number::Integer(i)) => Some(*i),
            Value::Number(Number::Float(f)) => f.to_i64(),
            Value::String(s) => parse_integer(s),
            Value::Boolean(b) => Some(i64::from(*b)),
            Value::Temporal(TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt)) => Some(dt.timestamp()),
            Value::Null | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => None,
        }
    }

    /// Interprets the value as a float, returning `None` when it cannot be represented as one.
    #[must_use]
    pub fn to_float(&self) -> Option<f64> {
        match self {
            Value::Number(Number::Integer(i)) => i.to_f64(),
            Value::Number(Number::Float(f)) => Some(*f),
            Value::String(s) => parse_float(s),
            Value::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Temporal(TemporalValue::DateTime(dt) | TemporalValue::Date(dt) | TemporalValue::Time(dt)) => dt
                .timestamp()
                .to_f64()
                .map(|secs| secs + f64::from(dt.timestamp_subsec_nanos()) / 1_000_000_000.0),
            Value::Null | Value::Geospatial(_) | Value::Array(_) | Value::Object(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value::{
        parsing::{parse_boolean, parse_float, parse_integer},
        types::{Number, Value},
    };
    use compact_str::CompactString;

    #[test]
    fn boolean_tokens_parse_and_unknown_tokens_do_not() {
        assert_eq!(parse_boolean("YES"), Some(true));
        assert_eq!(parse_boolean(" off "), Some(false));
        assert_eq!(parse_boolean("maybe"), None);
    }

    #[test]
    fn integers_parse_from_clean_and_quoted_and_comma_decimals() {
        assert_eq!(parse_integer("42"), Some(42));
        assert_eq!(parse_integer("\"7\""), Some(7));
        assert_eq!(parse_integer("7,9"), Some(7));
        assert_eq!(parse_integer("abc"), None);
    }

    #[test]
    fn floats_parse_with_comma_decimals() {
        assert_eq!(parse_float("1,5"), Some(1.5));
        assert_eq!(parse_float("nope"), None);
    }

    #[test]
    fn to_integer_is_none_for_a_non_numeric_value() {
        assert_eq!(Value::String(CompactString::from("hello")).to_integer(), None);
        assert_eq!(Value::Null.to_integer(), None);
    }

    #[test]
    fn to_boolean_reads_numbers_as_truthiness() {
        assert_eq!(Value::Number(Number::Integer(0)).to_boolean(), Some(false));
        assert_eq!(Value::Number(Number::Integer(3)).to_boolean(), Some(true));
    }
}
