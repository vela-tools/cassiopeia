//! The serde readers for the manifest fields that may read environment variables, each filling its
//! variables from this process's environment.

use crate::environment::{field::TemplatedField, resolution};
use cassiopeia_common::{environment::template::process_environment, input::Input, masked_url::MaskedUrl, request_header::RequestHeaders};
use http::HeaderName;
use serde::{
    Deserialize,
    Deserializer,
    de::{Error as DeError, MapAccess, Visitor},
};
use std::fmt;

/// Reads an input's `source`.
///
/// # Errors
/// Fails when the text is not a string or cannot be read, as
/// [`read_source`](resolution::read_source) describes.
pub fn source<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Input, D::Error> {
    let text = String::deserialize(deserializer)?;
    resolution::read_source(&text, process_environment).map_err(DeError::custom)
}

/// Reads the output's broker `url`.
///
/// # Errors
/// Fails when the text is not a string or cannot be read, as [`read_url`](resolution::read_url)
/// describes.
pub fn url<'de, D: Deserializer<'de>>(deserializer: D) -> Result<MaskedUrl, D::Error> {
    let text = String::deserialize(deserializer)?;
    resolution::read_url(&text, process_environment).map_err(DeError::custom)
}

/// Reads an input's `headers`, a map of header name to value.
///
/// # Errors
/// Fails when the map is malformed or a header cannot be read, as
/// [`read_header`](resolution::read_header) describes.
pub fn input_headers<'de, D: Deserializer<'de>>(deserializer: D) -> Result<RequestHeaders, D::Error> {
    deserializer.deserialize_map(HeadersVisitor {
        field: TemplatedField::InputHeader,
    })
}

/// Reads the output's `headers`, a map of header name to value.
///
/// # Errors
/// Fails when the map is malformed or a header cannot be read, as
/// [`read_header`](resolution::read_header) describes.
pub fn output_headers<'de, D: Deserializer<'de>>(deserializer: D) -> Result<RequestHeaders, D::Error> {
    deserializer.deserialize_map(HeadersVisitor {
        field: TemplatedField::OutputHeader,
    })
}

/// The visitor that reads a header map in order, filling each value's variables as it arrives.
struct HeadersVisitor {
    /// Names a header's place in the manifest, given its validated name.
    field: fn(HeaderName) -> TemplatedField,
}

impl<'de> Visitor<'de> for HeadersVisitor {
    type Value = RequestHeaders;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a map of HTTP header names to values")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<RequestHeaders, M::Error> {
        let mut headers = Vec::with_capacity(access.size_hint().unwrap_or_default());
        while let Some((name, value)) = access.next_entry::<String, String>()? {
            headers.push(resolution::read_header(&name, &value, self.field, process_environment).map_err(DeError::custom)?);
        }
        Ok(RequestHeaders::new(headers))
    }
}
