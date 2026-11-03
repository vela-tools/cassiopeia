use crate::environment::secret::{MaskStyle, Secrets};
use derive_more::Debug;
use http::header::{HeaderName, HeaderValue};
use serde::{
    Serialize,
    Serializer,
    ser::{Error as SerError, SerializeMap},
};
use std::str::FromStr;
use thiserror::Error;

/// A single user-supplied HTTP header attached to every request Cassiopeia sends to one endpoint:
/// a Context Broker it writes to, or a remote source it fetches.
///
/// Typically carries a credential the endpoint requires (an `Authorization: Bearer …` token, an
/// API-key header, or HTTP Basic) that Cassiopeia forwards verbatim without interpreting. The value
/// is treated as a secret: it never appears in `Debug` output, and
/// [`RequestHeaders::sensitive`] marks it sensitive on the outgoing request so it stays out of logs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RequestHeader {
    /// The header name, validated against the HTTP token grammar.
    name: HeaderName,
    /// The header value; skipped in `Debug` so a credential never reaches a log line.
    #[debug(skip)]
    value: HeaderValue,
    /// The values read from the environment into the header value, which serialization writes back
    /// as `${NAME}` rather than as the values themselves.
    #[debug(skip)]
    secrets: Secrets,
}

impl RequestHeader {
    /// Builds a header from a name and value, validating both against the HTTP grammar. Surrounding
    /// whitespace is trimmed, so a value split off a `Name: Value` string keeps no leading space.
    ///
    /// # Errors
    /// Returns [`RequestHeaderError::Name`] when `name` is not a valid header name, or
    /// [`RequestHeaderError::Value`] when `value` is not a valid header value. The rejected value is
    /// never echoed, since it may be a secret.
    pub fn new(name: &str, value: &str) -> Result<RequestHeader, RequestHeaderError> {
        let name = RequestHeader::parse_name(name)?;
        let value = HeaderValue::from_str(value.trim()).map_err(|_| RequestHeaderError::Value { name: name.to_string() })?;
        Ok(RequestHeader {
            name,
            value,
            secrets: Secrets::new(),
        })
    }

    /// Validates a header name against the HTTP token grammar, trimming surrounding whitespace.
    ///
    /// # Errors
    /// Returns [`RequestHeaderError::Name`] when `name` is not a valid header name.
    pub fn parse_name(name: &str) -> Result<HeaderName, RequestHeaderError> {
        HeaderName::from_str(name.trim()).map_err(|_| RequestHeaderError::Name { name: name.trim().to_owned() })
    }

    /// Records the values read from the environment into the header value.
    #[must_use]
    pub fn with_secrets(self, secrets: Secrets) -> RequestHeader {
        RequestHeader { secrets, ..self }
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

impl FromStr for RequestHeader {
    type Err = RequestHeaderError;

    /// Parses a header written as `Name: Value`, splitting on the first colon.
    fn from_str(s: &str) -> Result<RequestHeader, RequestHeaderError> {
        let (name, value) = s.split_once(':').ok_or(RequestHeaderError::Malformed)?;
        RequestHeader::new(name, value)
    }
}

/// The reason a request header could not be read.
///
/// None of these variants carry the header value, so a malformed or rejected credential is never
/// surfaced in an error message.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RequestHeaderError {
    /// The header was not written in `Name: Value` form.
    #[error("request header must be written as 'Name: Value'")]
    Malformed,
    /// The header name is not a valid HTTP token.
    #[error("invalid request header name: {name}")]
    Name {
        /// The offending header name.
        name: String,
    },
    /// The header value is not a valid HTTP header value.
    #[error("invalid value for request header '{name}'")]
    Value {
        /// The name of the header whose value was rejected.
        name: String,
    },
}

/// The ordered set of user-supplied headers attached to every request sent to one endpoint.
///
/// Serializes to a JSON object mapping each header name to its value, so a manifest spells
/// credentials as `"headers": {"Authorization": "Bearer …"}`, on an input and on a broker output
/// alike. The manifest reads that object back itself, since it fills each value's environment
/// variables as it goes.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct RequestHeaders(Vec<RequestHeader>);

impl RequestHeaders {
    /// Builds the set from a list of parsed headers.
    #[must_use]
    pub const fn new(headers: Vec<RequestHeader>) -> RequestHeaders {
        RequestHeaders(headers)
    }

    /// Returns whether the set holds no headers.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates over the headers in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &RequestHeader> {
        self.0.iter()
    }

    /// Iterates over the headers in insertion order as name and value pairs ready to attach to a
    /// request, every value marked sensitive so hyper keeps it out of connection logs.
    pub fn sensitive(&self) -> impl Iterator<Item = (HeaderName, HeaderValue)> {
        self.0.iter().map(|header| {
            // A request owns the headers it carries, while the set stays with its configuration.
            let mut value = header.value.clone();
            value.set_sensitive(true);
            (header.name.clone(), value)
        })
    }
}

impl Serialize for RequestHeaders {
    /// Writes the headers as a JSON object of name to value, each value as the template it reads back
    /// from: any value read from the environment as `${NAME}`, and any literal `${` escaped.
    ///
    /// # Errors
    /// Fails when a header value holds bytes that are not representable as a string. A value built
    /// from user text is always representable, so this cannot arise for headers this crate accepts.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for header in &self.0 {
            let value = header.value.to_str().map_err(SerError::custom)?;
            map.serialize_entry(header.name.as_str(), &header.secrets.mask(value, MaskStyle::Template))?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        environment::{
            secret::{Secret, Secrets},
            variable_name::VariableName,
        },
        request_header::{RequestHeader, RequestHeaderError, RequestHeaders},
    };
    use pretty_assertions::assert_eq;
    use secrecy::SecretString;
    use std::str::FromStr;

    #[test]
    fn a_name_and_value_build_a_validated_header() {
        let header = RequestHeader::new("Authorization", "Bearer token").unwrap();

        assert_eq!(header.name().as_str(), "authorization");
        assert_eq!(header.value().to_str().unwrap(), "Bearer token");
    }

    #[test]
    fn parsing_splits_on_the_first_colon_and_trims_the_value() {
        let header = RequestHeader::from_str("X-Api-Key: abc:def").unwrap();

        assert_eq!(header.name().as_str(), "x-api-key");
        assert_eq!(header.value().to_str().unwrap(), "abc:def");
    }

    #[test]
    fn parsing_without_a_colon_reports_a_malformed_header() {
        assert_eq!(RequestHeader::from_str("no-colon-here"), Err(RequestHeaderError::Malformed));
    }

    #[test]
    fn an_invalid_header_name_is_rejected() {
        assert!(matches!(RequestHeader::new("bad name", "value"), Err(RequestHeaderError::Name { .. })));
    }

    #[test]
    fn an_invalid_header_value_is_rejected_without_echoing_it() {
        let error = RequestHeader::new("Authorization", "secret\nwith-newline").unwrap_err();

        assert_eq!(
            error,
            RequestHeaderError::Value {
                name: "authorization".to_owned()
            }
        );
        assert!(!error.to_string().contains("secret"));
    }

    #[test]
    fn the_debug_output_never_contains_the_value() {
        let header = RequestHeader::new("Authorization", "Bearer supersecret").unwrap();

        let rendered = format!("{header:?}");
        assert!(!rendered.contains("supersecret"));
        assert!(rendered.contains("authorization"));
    }

    #[test]
    fn headers_serialize_as_a_json_object_in_order() {
        let headers = RequestHeaders::new(vec![
            RequestHeader::new("Authorization", "Bearer t").unwrap(),
            RequestHeader::new("X-Api-Key", "k").unwrap(),
        ]);

        assert_eq!(serde_json::to_string(&headers).unwrap(), r#"{"authorization":"Bearer t","x-api-key":"k"}"#);
    }

    #[test]
    fn a_header_value_serializes_its_secrets_as_placeholders_and_escapes_a_literal_opening() {
        let mut secrets = Secrets::new();
        secrets.insert(Secret::new(VariableName::from_str("BROKER_TOKEN").unwrap(), &SecretString::from("tok-81b")));
        let headers = RequestHeaders::new(vec![
            RequestHeader::new("Authorization", "Bearer tok-81b").unwrap().with_secrets(secrets),
            RequestHeader::new("X-Note", "${not-a-variable}").unwrap(),
        ]);

        assert_eq!(
            serde_json::to_string(&headers).unwrap(),
            r#"{"authorization":"Bearer ${BROKER_TOKEN}","x-note":"$${not-a-variable}"}"#
        );
    }

    #[test]
    fn the_default_header_set_is_empty() {
        assert!(RequestHeaders::default().is_empty());
    }

    #[test]
    fn sensitive_pairs_keep_the_order_and_mark_every_value_sensitive() {
        let headers = RequestHeaders::new(vec![
            RequestHeader::new("Authorization", "Bearer t").unwrap(),
            RequestHeader::new("X-Api-Key", "k").unwrap(),
        ]);

        let pairs: Vec<_> = headers.sensitive().collect();

        assert_eq!(
            pairs.iter().map(|(name, value)| (name.as_str(), value.to_str().unwrap())).collect::<Vec<_>>(),
            vec![("authorization", "Bearer t"), ("x-api-key", "k")]
        );
        assert!(pairs.iter().all(|(_, value)| value.is_sensitive()));
    }
}
