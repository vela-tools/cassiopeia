use crate::{detectors::Detector, profile::Profile};
use cassiopeia_common::format::DataFormat;
use mediatype::media_type;
use quick_xml::{Reader, events::Event};

/// Detects generic XML by its declaration or a well-formed first element.
///
/// This is the permissive, schemaless fallback for the markup family: it runs after the specific XML
/// dialects (KML) so those win, and its confidence is deliberately lower than theirs because "this is
/// XML" says nothing about which records it carries. The record shape is discovered later, in the
/// ingestor, from a deterministic convention rather than from any detected metadata, so no
/// format-specific metadata is attached here.
pub struct XmlDetector;

impl Detector for XmlDetector {
    fn detect(&self, bytes: &[u8]) -> Option<Profile> {
        if looks_like_xml(bytes) {
            return Some(Profile::new(DataFormat::Xml, media_type!(APPLICATION / XML), 0.6));
        }
        None
    }
}

/// Reports whether `bytes` open as XML: a `<?xml` declaration, or a `<` that a pull parser can read
/// through to a first element without error.
fn looks_like_xml(bytes: &[u8]) -> bool {
    let content = skip_bom_and_whitespace(bytes);
    if !content.starts_with(b"<") {
        return false;
    }
    // A declaration is a definitive marker; nothing else legally opens `<?xml`.
    if content.starts_with(b"<?xml") {
        return true;
    }

    let mut reader = Reader::from_reader(content);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            // A first element (with or without children) confirms markup.
            Ok(Event::Start(_) | Event::Empty(_)) => return true,
            // Prologue nodes may legally precede the root element; keep reading past them.
            Ok(Event::Decl(_) | Event::Comment(_) | Event::PI(_) | Event::DocType(_) | Event::Text(_)) => buf.clear(),
            // An end tag, stray CDATA, an entity reference (only legal inside element content), end
            // of input before any element, or a parse error means not XML.
            Ok(Event::End(_) | Event::CData(_) | Event::GeneralRef(_) | Event::Eof) | Err(_) => {
                return false;
            }
        }
    }
}

/// Skips a leading UTF-8 BOM and any XML whitespace, returning the remaining bytes.
fn skip_bom_and_whitespace(bytes: &[u8]) -> &[u8] {
    let bytes = match bytes.strip_prefix(&[0xEFu8, 0xBB, 0xBF]) {
        Some(rest) => rest,
        None => bytes,
    };
    let offset = bytes.iter().take_while(|byte| byte.is_ascii_whitespace()).count();
    &bytes[offset..]
}

#[cfg(test)]
mod tests {
    use crate::detectors::{Detector, xml::XmlDetector};
    use cassiopeia_common::format::DataFormat;

    #[test]
    fn a_declaration_and_empty_root_is_detected_as_xml() {
        let profile = XmlDetector.detect(br#"<?xml version="1.0"?><root/>"#).unwrap();
        assert_eq!(*profile.format(), DataFormat::Xml);
        assert_eq!(profile.mime_type().to_string(), "application/xml");
        assert!(profile.metadata().is_none());
    }

    #[test]
    fn a_bare_element_without_a_declaration_is_detected_as_xml() {
        let profile = XmlDetector.detect(b"<root>text</root>").unwrap();
        assert_eq!(*profile.format(), DataFormat::Xml);
    }

    #[test]
    fn leading_bom_and_whitespace_before_the_root_are_skipped() {
        let profile = XmlDetector.detect(b"\xEF\xBB\xBF\n  <root/>").unwrap();
        assert_eq!(*profile.format(), DataFormat::Xml);
    }

    #[test]
    fn a_json_object_is_not_detected_as_xml() {
        assert!(XmlDetector.detect(br#"{"a":1}"#).is_none());
    }

    #[test]
    fn non_markup_bytes_are_rejected() {
        assert!(XmlDetector.detect(&[0xffu8, 0x00, 0xfe]).is_none());
    }

    #[test]
    fn an_angle_bracket_followed_by_junk_is_rejected() {
        assert!(XmlDetector.detect(b"<@#$ not markup").is_none());
    }
}
