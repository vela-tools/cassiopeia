use derive_more::Debug;
use http::header::{HeaderName, HeaderValue};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{Error as DeError, MapAccess, Visitor},
    ser::{Error as SerError, SerializeMap},
};
use std::{fmt, str::FromStr};
use thiserror::Error;

/// A single HTTP header attached to every Context Broker request.
///
/// Carries a credential a broker requires (an `Authorization: Bearer …` token, an API-key header,
/// or HTTP Basic) that Cassiopeia forwards verbatim without interpreting. The value is treated as a
/// secret: it never appears in `Debug` output, and the broker writer marks it sensitive on the
/// outgoing request so it stays out of logs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BrokerHeader {
    /// The header name, validated against the HTTP token grammar.
    name: HeaderName,
    /// The header value; skipped in `Debug` so a credential never reaches a log line.
    #[debug(skip)]
    value: HeaderValue,
}

impl BrokerHeader {
    /// Builds a header from a name and value, validating both against the HTTP grammar. Surrounding
    /// whitespace is trimmed, so a value split off a `Name: Value` string keeps no leading space.
    ///
    /// # Errors
    /// Returns [`BrokerHeaderError::Name`] when `name` is not a valid header name, or
    /// [`BrokerHeaderError::Value`] when `value` is not a valid header value. The rejected value is
    /// never echoed, since it may be a secret.
    pub fn new(name: &str, value: &str) -> Result<BrokerHeader, BrokerHeaderError> {
        let name = HeaderName::from_str(name.trim()).map_err(|_| BrokerHeaderError::Name { name: name.trim().to_owned() })?;
        let value = HeaderValue::from_str(value.trim()).map_err(|_| BrokerHeaderError::Value { name: name.to_string() })?;
        Ok(BrokerHeader { name, value })
    }

    /// Returns the header name.
    #[must_use]
    pub const fn name(&self) -> &HeaderName {
        &self.name
    }

    /// Returns the header value.
    #[must_use]
    pub const fn value(&self) -> &HeaderValue {
        &self.value
    }
}

impl FromStr for BrokerHeader {
    type Err = BrokerHeaderError;

    /// Parses a header written as `Name: Value`, splitting on the first colon.
    fn from_str(s: &str) -> Result<BrokerHeader, BrokerHeaderError> {
        let (name, value) = s.split_once(':').ok_or(BrokerHeaderError::Malformed)?;
        BrokerHeader::new(name, value)
    }
}

/// The reason a broker header could not be read.
///
/// None of these variants carry the header value, so a malformed or rejected credential is never
/// surfaced in an error message.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BrokerHeaderError {
    /// The header was not written in `Name: Value` form.
    #[error("broker header must be written as 'Name: Value'")]
    Malformed,
    /// The header name is not a valid HTTP token.
    #[error("invalid broker header name: {name}")]
    Name {
        /// The offending header name.
        name: String,
    },
    /// The header value is not a valid HTTP header value.
    #[error("invalid value for broker header '{name}'")]
    Value {
        /// The name of the header whose value was rejected.
        name: String,
    },
}

/// The ordered set of user-supplied headers attached to every Context Broker request.
///
/// Serializes to, and deserializes from, a JSON object mapping each header name to its value, so a
/// manifest spells broker credentials as `"headers": {"Authorization": "Bearer …"}`.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct BrokerHeaders(Vec<BrokerHeader>);

impl BrokerHeaders {
    /// Builds the set from a list of parsed headers.
    #[must_use]
    pub const fn new(headers: Vec<BrokerHeader>) -> BrokerHeaders {
        BrokerHeaders(headers)
    }

    /// Returns whether the set holds no headers.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates over the headers in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &BrokerHeader> {
        self.0.iter()
    }
}

impl Serialize for BrokerHeaders {
    /// Writes the headers as a JSON object of name to value.
    ///
    /// # Errors
    /// Fails when a header value holds bytes that are not representable as a string. A value built
    /// from user text is always representable, so this cannot arise for headers this crate accepts.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for header in &self.0 {
            let value = header.value.to_str().map_err(SerError::custom)?;
            map.serialize_entry(header.name.as_str(), value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for BrokerHeaders {
    /// Reads a JSON object of header name to value, validating each pair.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<BrokerHeaders, D::Error> {
        deserializer.deserialize_map(BrokerHeadersVisitor)
    }
}

/// The visitor that reads a [`BrokerHeaders`] map, validating each entry as it arrives.
struct BrokerHeadersVisitor;

impl<'de> Visitor<'de> for BrokerHeadersVisitor {
    type Value = BrokerHeaders;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a map of HTTP header names to values")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<BrokerHeaders, M::Error> {
        let mut headers = Vec::with_capacity(access.size_hint().unwrap_or_default());
        while let Some((name, value)) = access.next_entry::<String, String>()? {
            headers.push(BrokerHeader::new(&name, &value).map_err(DeError::custom)?);
        }
        Ok(BrokerHeaders(headers))
    }
}

#[cfg(test)]
mod tests {
    use crate::broker_header::{BrokerHeader, BrokerHeaderError, BrokerHeaders};
    use std::str::FromStr;

    #[test]
    fn a_name_and_value_build_a_validated_header() {
        let header = BrokerHeader::new("Authorization", "Bearer token").unwrap();

        assert_eq!(header.name().as_str(), "authorization");
        assert_eq!(header.value().to_str().unwrap(), "Bearer token");
    }

    #[test]
    fn parsing_splits_on_the_first_colon_and_trims_the_value() {
        let header = BrokerHeader::from_str("X-Api-Key: abc:def").unwrap();

        assert_eq!(header.name().as_str(), "x-api-key");
        assert_eq!(header.value().to_str().unwrap(), "abc:def");
    }

    #[test]
    fn parsing_without_a_colon_reports_a_malformed_header() {
        assert_eq!(BrokerHeader::from_str("no-colon-here"), Err(BrokerHeaderError::Malformed));
    }

    #[test]
    fn an_invalid_header_name_is_rejected() {
        assert!(matches!(BrokerHeader::new("bad name", "value"), Err(BrokerHeaderError::Name { .. })));
    }

    #[test]
    fn an_invalid_header_value_is_rejected_without_echoing_it() {
        let error = BrokerHeader::new("Authorization", "secret\nwith-newline").unwrap_err();

        assert_eq!(
            error,
            BrokerHeaderError::Value {
                name: "authorization".to_owned()
            }
        );
        assert!(!error.to_string().contains("secret"));
    }

    #[test]
    fn the_debug_output_never_contains_the_value() {
        let header = BrokerHeader::new("Authorization", "Bearer supersecret").unwrap();

        let rendered = format!("{header:?}");
        assert!(!rendered.contains("supersecret"));
        assert!(rendered.contains("authorization"));
    }

    #[test]
    fn headers_round_trip_through_a_json_object() {
        let headers: BrokerHeaders = serde_json::from_str(r#"{"Authorization": "Bearer t", "X-Api-Key": "k"}"#).unwrap();

        assert_eq!(headers.iter().count(), 2);
        let encoded = serde_json::to_string(&headers).unwrap();
        assert_eq!(serde_json::from_str::<BrokerHeaders>(&encoded).unwrap(), headers);
    }

    #[test]
    fn a_malformed_header_map_entry_fails_deserialization() {
        assert!(serde_json::from_str::<BrokerHeaders>(r#"{"bad name": "value"}"#).is_err());
    }

    #[test]
    fn the_default_header_set_is_empty() {
        assert!(BrokerHeaders::default().is_empty());
    }
}
