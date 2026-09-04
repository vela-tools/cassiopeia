use derive_more::{AsRef, Display, From};

/// An RFC 6901 JSON Pointer naming one location inside a JSON document.
///
/// Validation engines report where an instance broke a schema and where in the schema the rule
/// lived; both are pointers, and both reach the reporting layer as this newtype rather than as bare
/// text, so a diagnostic cannot confuse one with a message or a name.
#[derive(AsRef, Clone, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
#[as_ref(str)]
pub struct JsonPointer(Box<str>);

impl JsonPointer {
    /// Builds a pointer from its rendered form.
    #[must_use]
    pub fn new(pointer: impl Into<Box<str>>) -> JsonPointer {
        JsonPointer(pointer.into())
    }

    /// The pointer's rendered form.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the pointer addresses the document root, which carries no segments.
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The first reference token, with its leading `/` removed.
    ///
    /// An NGSI-LD entity holds every attribute as a top-level member (ETSI GS CIM 009 v1.9.1 clause
    /// 4.5.1), so the first token of an instance pointer names the attribute a violation sits under.
    #[must_use]
    pub fn first_segment(&self) -> Option<&str> {
        self.0.strip_prefix('/')?.split('/').next().filter(|segment| !segment.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use crate::json_pointer::JsonPointer;

    #[test]
    fn the_root_pointer_carries_no_segment() {
        let pointer = JsonPointer::new("");

        assert!(pointer.is_root());
        assert_eq!(pointer.first_segment(), None);
    }

    #[test]
    fn the_first_segment_names_the_top_level_member() {
        assert_eq!(JsonPointer::new("/temperature/value").first_segment(), Some("temperature"));
        assert_eq!(JsonPointer::new("/dateObserved").first_segment(), Some("dateObserved"));
    }

    #[test]
    fn a_pointer_renders_its_own_text() {
        assert_eq!(JsonPointer::new("/properties/mass").to_string(), "/properties/mass");
    }
}
