use crate::{encoding_evidence::evidence_lines, top_level_domain::TopLevelDomain};
use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, UTF_8};
use mediatype::{MediaTypeBuf, ReadParams, names::CHARSET};
use url::Url;

/// What a source says about its character encoding apart from its bytes.
///
/// Content alone cannot always settle a legacy single-byte encoding: a German table whose only
/// non-ASCII bytes are `°`, `µ`, and `³` in windows-1252 reads just as well as the letters `ľ` and `ł`
/// in ISO-8859-2. A remote source supplies the context that settles it: a `charset` its server
/// declares, and the top-level domain it is served from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncodingHint {
    /// The encoding the server declared in the `charset` parameter of its `Content-Type`.
    declared: Option<&'static Encoding>,
    /// The top-level domain the source was served from.
    top_level_domain: Option<TopLevelDomain>,
}

impl EncodingHint {
    /// No hint: a local file, or an in-memory buffer, whose encoding is guessed from content alone.
    pub const NONE: EncodingHint = EncodingHint {
        declared: None,
        top_level_domain: None,
    };

    /// The hint a source fetched from `url` carries, given the `Content-Type` its server answered with.
    ///
    /// A `charset` parameter `encoding_rs` does not recognise declares nothing, and the encoding is
    /// detected as for an undeclared source.
    #[must_use]
    pub fn for_remote_source(url: &Url, content_type: Option<&MediaTypeBuf>) -> EncodingHint {
        let declared = content_type
            .and_then(|content_type| content_type.get_param(CHARSET))
            .and_then(|charset| Encoding::for_label(charset.unquoted_str().as_bytes()));
        EncodingHint {
            declared,
            top_level_domain: TopLevelDomain::of(url),
        }
    }

    /// The encoding of a source whose content is `bytes`, all of it.
    ///
    /// A byte order mark wins over everything else: the WHATWG Encoding Standard's `decode` algorithm
    /// (clause 6) sniffs it ahead of any label, and the ingestor's decoder honours it the same way,
    /// so a UTF-16 file with a BOM is reported as the encoding it is read in. Otherwise a declared
    /// `charset` wins: the `Content-Type` parameter defines the encoding of a `text/csv` body
    /// (RFC 4180 clause 3, RFC 9110 clause 8.3.2). Content that is valid UTF-8 throughout is UTF-8,
    /// pure ASCII included. Otherwise the encoding is detected from the content,
    /// with the top-level domain weighing the guess toward the encodings native to its region, as a
    /// browser weighs an undeclared page. ISO-2022-JP detection is denied, since it is only reliable
    /// in a browser context.
    ///
    /// The whole content is weighed, never a leading sample: a file whose first rows are plain ASCII
    /// often carries its only accented names further down, and a guess from the ASCII alone would
    /// misread every one of them. The detector is fed the content's [`evidence_lines`], the lines
    /// holding a non-ASCII byte, drawn from across the whole file and capped, which keeps detection
    /// to well under a second however large the file.
    #[must_use]
    pub fn encoding_of(&self, bytes: &[u8]) -> &'static Encoding {
        if let Some((encoding, _)) = Encoding::for_bom(bytes) {
            return encoding;
        }
        if let Some(declared) = self.declared {
            return declared;
        }
        if str::from_utf8(bytes).is_ok() {
            return UTF_8;
        }
        let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
        evidence_lines(bytes).for_each(|line| {
            detector.feed(line, false);
        });
        detector.feed(&[], true);
        detector.guess(self.top_level_domain.as_ref().map(TopLevelDomain::as_bytes), Utf8Detection::Deny)
    }
}

#[cfg(test)]
mod tests {
    use crate::encoding_hint::EncodingHint;
    use encoding_rs::{ISO_8859_2, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};
    use mediatype::MediaTypeBuf;
    use pretty_assertions::assert_eq;
    use url::Url;

    /// The shape of Vienna's air-quality units row in windows-1252: `°C`, `µg/m³`, and `mg/m³`.
    const WINDOWS_1252_UNITS: &[u8] =
        b";MESZ;\xb0C;MESZ;\xb5g/m\xb3;MESZ;\xb5g/m\xb3;mg/m\xb3\nSTEF;30.10.2026, 10:00;18,48;30.10.2026, 10:00;12,48;30.10.2026, 10:00;9,1;0,14\n";

    /// Genuine Central European text in ISO-8859-2: `Žižkov Łódź Košice`.
    const ISO_8859_2_NAMES: &[u8] = b"id;name\n1;\xaei\xbekov \xa3\xf3d\xbc Ko\xb9ice\n";

    fn remote(url: &str, content_type: Option<&str>) -> EncodingHint {
        let content_type = content_type.map(|content_type| content_type.parse::<MediaTypeBuf>().unwrap());
        EncodingHint::for_remote_source(&Url::parse(url).unwrap(), content_type.as_ref())
    }

    #[test]
    fn without_a_hint_symbols_alone_are_guessed_as_central_european_letters() {
        // The ambiguity a hint exists to settle: content alone reads the units as ISO-8859-2.
        assert_eq!(EncodingHint::NONE.encoding_of(WINDOWS_1252_UNITS), ISO_8859_2);
    }

    #[test]
    fn an_austrian_domain_settles_unit_symbols_as_windows_1252() {
        let hint = remote("https://www.wien.gv.at/ma22-lgb/umweltgut/lumesakt-v2.csv", Some("text/csv"));

        assert_eq!(hint.encoding_of(WINDOWS_1252_UNITS), WINDOWS_1252);
    }

    #[test]
    fn a_central_european_domain_keeps_iso_8859_2_letters() {
        let hint = remote("https://data.example.si/feed.csv", None);

        assert_eq!(hint.encoding_of(ISO_8859_2_NAMES), ISO_8859_2);
    }

    #[test]
    fn a_declared_charset_wins_over_the_domain_and_the_content() {
        let hint = remote("https://www.wien.gv.at/feed.csv", Some("text/csv; charset=\"ISO-8859-2\""));

        assert_eq!(hint.encoding_of(WINDOWS_1252_UNITS), ISO_8859_2);
    }

    #[test]
    fn an_unrecognised_charset_declares_nothing() {
        let hint = remote("https://www.wien.gv.at/feed.csv", Some("text/csv; charset=x-unknown"));

        assert_eq!(hint.encoding_of(WINDOWS_1252_UNITS), WINDOWS_1252);
    }

    /// `id;name\n1;Café\n` in UTF-16LE behind its byte order mark.
    fn utf_16le_with_bom() -> Vec<u8> {
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend("id;name\n1;Café\n".encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }

    #[test]
    fn a_utf_16le_byte_order_mark_is_read_as_utf_16le_without_a_hint() {
        assert_eq!(EncodingHint::NONE.encoding_of(&utf_16le_with_bom()), UTF_16LE);
    }

    #[test]
    fn a_utf_16be_byte_order_mark_is_read_as_utf_16be_without_a_hint() {
        let mut bytes = vec![0xfe, 0xff];
        bytes.extend("id;name\n1;Café\n".encode_utf16().flat_map(u16::to_be_bytes));

        assert_eq!(EncodingHint::NONE.encoding_of(&bytes), UTF_16BE);
    }

    #[test]
    fn a_byte_order_mark_wins_over_a_declared_charset() {
        let hint = remote("https://www.wien.gv.at/feed.csv", Some("text/csv; charset=windows-1252"));

        assert_eq!(hint.encoding_of(&utf_16le_with_bom()), UTF_16LE);
    }

    #[test]
    fn a_utf_8_byte_order_mark_is_read_as_utf_8() {
        assert_eq!(EncodingHint::NONE.encoding_of(b"\xef\xbb\xbfid;name\n1;Caf\xc3\xa9\n"), UTF_8);
    }

    #[test]
    fn valid_utf_8_is_read_as_utf_8_whatever_the_domain() {
        let hint = remote("https://www.wien.gv.at/feed.csv", None);

        assert_eq!(hint.encoding_of("Station;µg/m³\n".as_bytes()), UTF_8);
    }
}
