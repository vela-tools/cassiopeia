use derive_more::{AsRef, Display, From};

/// The peer's own explanation of a failure.
///
/// RFC 7807 section 3.1 defines `detail` as human-readable prose specific to one occurrence, and ETSI
/// GS CIM 009 v1.9.1 clause 5.5.3 requires a broker to convey enough information there to act on. It
/// is unstructured text by definition, so it is carried as a newtype rather than parsed.
#[derive(AsRef, Clone, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
#[as_ref(str)]
pub struct Detail(Box<str>);

impl Detail {
    /// Builds a detail from the peer's text.
    #[must_use]
    pub fn new(detail: impl Into<Box<str>>) -> Detail {
        Detail(detail.into())
    }

    /// The detail's text.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::detail::Detail;

    #[test]
    fn a_detail_renders_the_text_it_was_given() {
        assert_eq!(
            Detail::new("attribute 'dateObserved' is not a valid DateTime").as_str(),
            "attribute 'dateObserved' is not a valid DateTime"
        );
    }
}
