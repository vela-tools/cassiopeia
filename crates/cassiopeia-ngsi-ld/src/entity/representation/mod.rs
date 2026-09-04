use crate::entity::error::Result as NgsiLdResult;
use cassiopeia_common::{representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
use serde::{Serialize, Serializer};
use serde_json::Value as JsonValue;
use std::fmt::Display;

pub mod concise;
pub mod normalized;
pub mod qualifiers;
pub mod simplified;

/// Whether serialized JSON is emitted compact or pretty-printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonLayout {
    /// A single dense line.
    Compact,
    /// Indented, multi-line output.
    Pretty,
}

/// Serializes a domain value in a specific NGSI-LD representation and null-handling mode.
///
/// Implemented by the entity and every attribute type so the whole model streams straight into a
/// [`serde::Serializer`], with no intermediate `serde_json::Value` tree and no per-key `String`
/// allocation.
pub trait SerializeRepr {
    /// Serializes `self` in the given representation and null-handling mode.
    ///
    /// # Errors
    /// Returns the serializer's error if a value cannot be serialized.
    fn serialize_repr<S: Serializer>(&self, serializer: S, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<S::Ok, S::Error>;
}

/// Adapts a [`SerializeRepr`] value into a plain [`serde::Serialize`], carrying the representation
/// and null-handling so any serializer (`sonic-rs`, `serde_json`) can stream it directly.
pub struct ReprAdapter<'a, T: SerializeRepr + ?Sized> {
    inner: &'a T,
    representation: NgsiLdRepresentation,
    skip_null: NgsiLdSkipNull,
}

impl<'a, T: SerializeRepr + ?Sized> ReprAdapter<'a, T> {
    /// Pairs a value with the representation and null-handling to serialize it under.
    #[must_use]
    pub const fn new(inner: &'a T, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> ReprAdapter<'a, T> {
        ReprAdapter {
            inner,
            representation,
            skip_null,
        }
    }
}

impl<T: SerializeRepr + ?Sized> Serialize for ReprAdapter<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.inner.serialize_repr(serializer, self.representation, self.skip_null)
    }
}

/// Serializes an entity (or a list of them) to NGSI-LD JSON.
pub trait NgsiLdSerializable {
    /// Serializes to a JSON value.
    ///
    /// # Errors
    /// Returns [`NgsiLdError`](crate::entity::error::NgsiLdError) if serialization fails.
    fn to_json(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> NgsiLdResult<JsonValue>;

    /// Serializes to a JSON string in the requested layout.
    ///
    /// # Errors
    /// Returns [`NgsiLdError`](crate::entity::error::NgsiLdError) if serialization fails.
    fn to_string(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull, layout: JsonLayout) -> NgsiLdResult<String>;
}

/// Serializes a [`Display`] value as a JSON string via `collect_str`, so validated domain newtypes
/// (URNs, IRIs, names, unit codes) reach the wire as their canonical string form without an
/// intermediate `String` allocation.
pub(crate) struct DisplayStr<T: Display>(pub(crate) T);

impl<T: Display> Serialize for DisplayStr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// Serializes a slice of [`Display`] values as a JSON array of their string forms, without
/// allocating an intermediate `Vec<String>`.
pub(crate) struct DisplaySeq<'a, T: Display>(pub(crate) &'a [T]);

impl<T: Display> Serialize for DisplaySeq<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(DisplayStr))
    }
}
