use derive_more::Display;

/// The document-naming part of a `$ref`: everything before any `#fragment`.
///
/// A `$ref` such as `Point.json#/properties/coordinates` names a document (`Point.json`) and a
/// pointer into it (`/properties/coordinates`); this newtype carries the document part so a resolver
/// cannot be handed a raw fragment or an unrelated string by mistake. An empty reference names the
/// document currently being expanded rather than another one. Borrowed rather than owned so the hot
/// expansion path does not allocate a copy of every reference it walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[display("{_0}")]
pub struct DocumentRef<'a>(&'a str);

impl<'a> DocumentRef<'a> {
    /// Wraps the document-naming part of a reference.
    #[must_use]
    pub const fn new(reference: &'a str) -> DocumentRef<'a> {
        DocumentRef(reference)
    }

    /// The reference as a string slice.
    #[must_use]
    pub const fn as_str(self) -> &'a str {
        self.0
    }

    /// Whether the reference names the document currently being expanded rather than another one.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::dereference::document_ref::DocumentRef;

    #[test]
    fn a_reference_exposes_its_string_and_emptiness() {
        assert_eq!(DocumentRef::new("Point.json").as_str(), "Point.json");
        assert!(!DocumentRef::new("Point.json").is_empty());
        assert!(DocumentRef::new("").is_empty());
    }

    #[test]
    fn a_reference_displays_as_its_string() {
        assert_eq!(DocumentRef::new("dataModel.OCF/Sensor.json").to_string(), "dataModel.OCF/Sensor.json");
    }
}
