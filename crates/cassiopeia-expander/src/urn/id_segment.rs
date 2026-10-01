use crate::urn::cleaner::Cleaner;

/// A cleaned, non-empty entity identifier: the `<id>` in `urn:ngsi-ld:<Type>:<id>`.
///
/// An NGSI-LD entity id is a URI (ETSI GS CIM 009 v1.9.1 clause 4.4.1), and a URN's
/// namespace-specific string must carry a value after the type (RFC 8141 clause 2), so an empty
/// segment would mint the invalid `urn:ngsi-ld:<Type>:`. The only constructors refuse to produce an
/// empty segment, so the URN builder cannot be handed one.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct IdSegment(String);

impl IdSegment {
    /// Cleans a raw identifier into a segment, or yields `None` when nothing URN-safe survives.
    ///
    /// Raw text made only of characters the [`Cleaner`] drops (`"!!"`, say) is as absent as empty
    /// text: both would leave the identifier segment empty.
    pub(crate) fn clean(raw_id: &str) -> Option<IdSegment> {
        let cleaned = Cleaner::clean(raw_id);
        if cleaned.is_empty() { None } else { Some(IdSegment(cleaned)) }
    }

    /// Appends a deduplication counter, joined by a single `-` unless the segment already ends in one.
    pub(crate) fn with_suffix(self, count: usize) -> IdSegment {
        let IdSegment(mut segment) = self;
        if !segment.ends_with('-') {
            segment.push('-');
        }
        segment.push_str(&count.to_string());
        IdSegment(segment)
    }

    /// The segment's text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::urn::id_segment::IdSegment;

    #[test]
    fn a_clean_identifier_becomes_a_segment_unchanged() {
        assert_eq!(IdSegment::clean("E7W").unwrap().as_str(), "E7W");
    }

    #[test]
    fn disallowed_characters_are_cleaned_out_of_the_segment() {
        assert_eq!(IdSegment::clean("E 7/W").unwrap().as_str(), "E7W");
    }

    #[test]
    fn empty_text_yields_no_segment() {
        assert_eq!(IdSegment::clean(""), None);
    }

    #[test]
    fn text_made_only_of_disallowed_characters_yields_no_segment() {
        assert_eq!(IdSegment::clean("\"\""), None);
        assert_eq!(IdSegment::clean(" !? "), None);
    }

    #[test]
    fn a_suffix_is_joined_with_a_dash() {
        assert_eq!(IdSegment::clean("Sensor").unwrap().with_suffix(2).as_str(), "Sensor-2");
    }

    #[test]
    fn a_suffix_reuses_a_trailing_dash() {
        assert_eq!(IdSegment::clean("Sensor-").unwrap().with_suffix(3).as_str(), "Sensor-3");
    }
}
