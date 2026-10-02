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
    /// Cleans a raw identifier into a segment, or yields `None` when no letter or digit survives.
    ///
    /// Raw text made only of characters the [`Cleaner`] drops (`"!!"`, say) is as absent as empty
    /// text: both would leave the identifier segment empty. So is text that cleans to separators
    /// alone: a placeholder such as `-` or `—` (transliterated to `--`) names no record, and minting
    /// it would merge every record carrying that placeholder into one `urn:ngsi-ld:<Type>:--`.
    pub(crate) fn clean(raw_id: &str) -> Option<IdSegment> {
        let cleaned = Cleaner::clean(raw_id);
        if cleaned.chars().any(char::is_alphanumeric) {
            Some(IdSegment(cleaned))
        } else {
            None
        }
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
    fn text_that_cleans_to_separators_alone_yields_no_segment() {
        assert_eq!(IdSegment::clean("-"), None);
        assert_eq!(IdSegment::clean("_"), None);
        assert_eq!(IdSegment::clean("- _ -"), None);
    }

    #[test]
    fn a_dash_placeholder_transliterated_to_separators_yields_no_segment() {
        assert_eq!(IdSegment::clean("—"), None);
        assert_eq!(IdSegment::clean("–"), None);
    }

    #[test]
    fn separators_beside_a_letter_or_digit_are_kept_in_the_segment() {
        assert_eq!(IdSegment::clean("-a").unwrap().as_str(), "-a");
        assert_eq!(IdSegment::clean("_7_").unwrap().as_str(), "_7_");
        assert_eq!(IdSegment::clean("A—B").unwrap().as_str(), "A--B");
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
