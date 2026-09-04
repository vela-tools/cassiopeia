use crate::xml::error::XmlIngestError;
use encoding_rs::Encoding;
use indexmap::IndexMap;
use quick_xml::{
    Error,
    Reader,
    XmlVersion,
    escape::{EscapeError, resolve_predefined_entity},
    events::{BytesPI, BytesRef, BytesStart, BytesText, Event},
};
use serde_json::{Map, Value};
use std::borrow::Cow;

/// Folds an XML document into a `serde_json` value under Cassiopeia's XML convention.
///
/// The result is always an object keyed by the single root element name, optionally carrying a `#pi`
/// entry for top-level processing instructions. Comments are dropped; CDATA, namespaces (kept as
/// prefixes), mixed content, and processing instructions are preserved.
///
/// The fold is schema-less: a name occurring once becomes an object or string and a name occurring N
/// times becomes an array, because XML cannot distinguish "one" from "an array of one" without a
/// schema. External and DTD entities are never expanded, so the fold is XXE-safe.
///
/// # Errors
///
/// Returns [`XmlIngestError`] when the input is not well-formed XML, references an unknown custom
/// entity, or contains no root element.
pub fn document_to_value(bytes: &[u8]) -> Result<Value, XmlIngestError> {
    // Since quick-xml 0.42 the core reader operates on UTF-8 only, so a non-UTF-8 source (declared
    // via a BOM or the XML `encoding=` attribute) is transcoded up front rather than during parsing.
    let decoded = decode_to_utf8(bytes)?;
    let mut reader = Reader::from_reader(decoded.as_bytes());
    let mut buf = Vec::new();
    let mut stack: Vec<ElementBuilder> = Vec::new();
    let mut root: Option<(String, Value)> = None;
    let mut document_pis: Vec<Value> = Vec::new();

    loop {
        let event = reader.read_event_into(&mut buf)?;
        match event {
            Event::Start(start) => stack.push(element_builder(&start)?),
            Event::Empty(start) => {
                let (name, value) = element_builder(&start)?.finish();
                attach(&mut stack, &mut root, name, value);
            }
            Event::End(_) => {
                // Well-formedness (checked by the reader) guarantees a matching open element.
                if let Some(builder) = stack.pop() {
                    let (name, value) = builder.finish();
                    attach(&mut stack, &mut root, name, value);
                }
            }
            Event::Text(text) => append_text(&mut stack, &text),
            // Since quick-xml 0.42 character and entity references are delivered as their own event
            // rather than embedded in the surrounding text. Numeric and predefined references are
            // resolved here; any other name is a custom entity, which is never expanded (XXE-safe).
            Event::GeneralRef(reference) => append_entity_ref(&mut stack, &reference)?,
            Event::CData(cdata) => {
                // Since quick-xml 0.42 the reader yields UTF-8 content directly; only XML 1.0 EOL
                // normalization remains, and CDATA is never entity-escaped.
                let content = cdata.xml10_content();
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&content);
                }
            }
            Event::PI(pi) => {
                let value = pi_value(&pi);
                match stack.last_mut() {
                    Some(top) => top.pis.push(value),
                    None => document_pis.push(value),
                }
            }
            // The declaration's encoding was already honoured while transcoding to UTF-8.
            Event::Comment(_) | Event::Decl(_) | Event::DocType(_) => {}
            Event::Eof => break,
        }
        buf.clear();
    }

    let (root_name, root_value) = root.ok_or(XmlIngestError::EmptyDocument)?;
    let mut document = Map::new();
    document.insert(root_name, root_value);
    if !document_pis.is_empty() {
        document.insert("#pi".to_owned(), Value::Array(document_pis));
    }
    Ok(Value::Object(document))
}

/// Transcodes the raw input to UTF-8 using the encoding a BOM or the XML declaration names.
///
/// A leading BOM wins; failing that, the `encoding=` attribute of the XML declaration is consulted;
/// with neither present the input is assumed to be UTF-8. Malformed bytes for the chosen encoding are
/// rejected rather than replaced, so no lossy `U+FFFD` ever reaches the fold.
fn decode_to_utf8(bytes: &[u8]) -> Result<Cow<'_, str>, XmlIngestError> {
    // A BOM is stripped here so it is not re-emitted as a `U+FEFF` in the decoded text.
    let (encoding, body) = match Encoding::for_bom(bytes) {
        Some((encoding, bom_len)) => (encoding, &bytes[bom_len..]),
        None => (declared_encoding(bytes).unwrap_or(encoding_rs::UTF_8), bytes),
    };
    encoding
        .decode_without_bom_handling_and_without_replacement(body)
        .ok_or_else(|| XmlIngestError::Decode(encoding.name()))
}

/// Reads the encoding named by the XML declaration, if the document opens with one naming a label
/// that maps to a supported encoding. The declaration is ASCII, so it decodes before any non-ASCII
/// content the parser would otherwise choke on.
fn declared_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    let mut reader = Reader::from_reader(bytes);
    let mut buf = Vec::new();
    let Ok(Event::Decl(declaration)) = reader.read_event_into(&mut buf) else {
        return None;
    };
    let label = declaration.encoding()?.ok()?;
    Encoding::for_label(label.as_bytes())
}

/// An element under construction as the pull parser descends through its children.
struct ElementBuilder {
    name: String,
    attributes: Vec<(String, Value)>,
    children: Vec<(String, Value)>,
    text: String,
    pis: Vec<Value>,
}

impl ElementBuilder {
    /// Starts an element from its decoded name and `@`-prefixed attributes.
    const fn new(name: String, attributes: Vec<(String, Value)>) -> ElementBuilder {
        ElementBuilder {
            name,
            attributes,
            children: Vec::new(),
            text: String::new(),
            pis: Vec::new(),
        }
    }

    /// Collapses the element into its name and value.
    ///
    /// An element with no attributes, children, or processing instructions is a leaf: its trimmed text
    /// when non-empty, otherwise `Null` (an empty element). Anything richer becomes an object holding
    /// `@`-prefixed attributes, a `#text` entry, the grouped child elements, and a `#pi` array.
    fn finish(self) -> (String, Value) {
        let ElementBuilder {
            name,
            attributes,
            children,
            text,
            pis,
        } = self;
        let trimmed = text.trim();
        if attributes.is_empty() && children.is_empty() && pis.is_empty() {
            let value = if trimmed.is_empty() { Value::Null } else { Value::String(trimmed.to_owned()) };
            return (name, value);
        }

        let mut map = Map::new();
        for (key, value) in attributes {
            map.insert(key, value);
        }
        if !trimmed.is_empty() {
            map.insert("#text".to_owned(), Value::String(trimmed.to_owned()));
        }
        for (child_name, value) in group_children(children) {
            map.insert(child_name, value);
        }
        if !pis.is_empty() {
            map.insert("#pi".to_owned(), Value::Array(pis));
        }
        (name, Value::Object(map))
    }
}

/// Groups child elements by name in document order, collapsing a name that repeats into an array.
///
/// A single occurrence stays an object or string and a repeated name becomes an array: the
/// record-splitting convention keys off exactly this "a child element repeated" signal.
fn group_children(children: Vec<(String, Value)>) -> Vec<(String, Value)> {
    let mut groups: IndexMap<String, Vec<Value>> = IndexMap::new();
    for (name, value) in children {
        groups.entry(name).or_default().push(value);
    }
    groups
        .into_iter()
        .map(|(name, mut values)| match values.len() {
            1 => match values.pop() {
                Some(single) => (name, single),
                None => (name, Value::Null),
            },
            _ => (name, Value::Array(values)),
        })
        .collect()
}

/// Builds an element from its start tag: its name plus `@`-prefixed attribute values.
fn element_builder(start: &BytesStart) -> Result<ElementBuilder, XmlIngestError> {
    let name = start.name().as_ref().to_owned();
    let mut attributes = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute?;
        let key = attribute.key.as_ref().to_owned();
        // `unescape_value` is unavailable under the `encoding` feature; `normalized_value` performs
        // the same XML 1.0 attribute-value normalization and predefined-entity resolution.
        let value = attribute.normalized_value(XmlVersion::Implicit1_0).map_err(map_value_error)?;
        attributes.push((format!("@{key}"), Value::String(value.into_owned())));
    }
    Ok(ElementBuilder::new(name, attributes))
}

/// Appends element text to the open element, ignoring text outside any element.
///
/// Since quick-xml 0.42 the reader yields UTF-8 content directly and delivers entity references as
/// their own events, so only XML 1.0 EOL normalization is applied here.
fn append_text(stack: &mut [ElementBuilder], text: &BytesText) {
    if let Some(top) = stack.last_mut() {
        top.text.push_str(&text.xml10_content());
    }
}

/// Resolves a character or entity reference and appends it to the open element.
///
/// Numeric references (`&#49;`, `&#x30;`) and the five predefined entities (`&lt;`, `&gt;`, `&amp;`,
/// `&apos;`, `&quot;`) are resolved to their characters. Any other name is a custom entity, which is
/// never expanded so the fold stays XXE and billion-laughs safe; it surfaces as
/// [`XmlIngestError::UnknownEntity`]. References outside any element are ignored, as bare text is.
fn append_entity_ref(stack: &mut [ElementBuilder], reference: &BytesRef) -> Result<(), XmlIngestError> {
    let Some(top) = stack.last_mut() else {
        return Ok(());
    };
    if let Some(character) = reference.resolve_char_ref().map_err(map_value_error)? {
        top.text.push(character);
        return Ok(());
    }
    match resolve_predefined_entity(reference) {
        Some(resolved) => top.text.push_str(resolved),
        None => return Err(XmlIngestError::UnknownEntity(reference.to_string())),
    }
    Ok(())
}

/// Builds a `{ "target": ..., "content": ... }` object for one processing instruction.
///
/// `quick_xml` reports the content with a leading space, which is trimmed off here.
fn pi_value(pi: &BytesPI) -> Value {
    let mut map = Map::new();
    map.insert("target".to_owned(), Value::String(pi.target().to_owned()));
    map.insert("content".to_owned(), Value::String(pi.content().trim().to_owned()));
    Value::Object(map)
}

/// Attaches a finished element to its parent, or records it as the document root when the stack is empty.
fn attach(stack: &mut [ElementBuilder], root: &mut Option<(String, Value)>, name: String, value: Value) {
    match stack.last_mut() {
        Some(parent) => parent.children.push((name, value)),
        None => *root = Some((name, value)),
    }
}

/// Maps a decode/unescape failure, surfacing an unknown custom entity as its own error.
fn map_value_error(error: Error) -> XmlIngestError {
    match error {
        Error::Escape(EscapeError::UnrecognizedEntity(_, name)) => XmlIngestError::UnknownEntity(name),
        error @ (Error::Io(_)
        | Error::Syntax(_)
        | Error::IllFormed(_)
        | Error::InvalidAttr(_)
        | Error::Encoding(_)
        | Error::Escape(_)
        | Error::Namespace(_)) => XmlIngestError::Parse(error),
    }
}

#[cfg(test)]
mod tests {
    use crate::xml::{error::XmlIngestError, value::document_to_value};
    use serde_json::json;

    #[test]
    fn a_pure_text_leaf_becomes_a_bare_string() {
        assert_eq!(document_to_value(b"<x>hello</x>").unwrap(), json!({ "x": "hello" }));
    }

    #[test]
    fn a_self_closing_element_becomes_null() {
        assert_eq!(document_to_value(b"<x/>").unwrap(), json!({ "x": null }));
    }

    #[test]
    fn an_empty_paired_element_becomes_null() {
        assert_eq!(document_to_value(b"<x></x>").unwrap(), json!({ "x": null }));
    }

    #[test]
    fn an_element_with_an_attribute_and_text_becomes_an_object() {
        assert_eq!(
            document_to_value(br#"<t unit="degC">24</t>"#).unwrap(),
            json!({ "t": { "#text": "24", "@unit": "degC" } })
        );
    }

    #[test]
    fn nested_elements_become_a_nested_object() {
        assert_eq!(document_to_value(b"<a><b>1</b></a>").unwrap(), json!({ "a": { "b": "1" } }));
    }

    #[test]
    fn a_single_child_occurrence_is_an_object_not_an_array() {
        assert_eq!(document_to_value(b"<a><item>1</item></a>").unwrap(), json!({ "a": { "item": "1" } }));
    }

    #[test]
    fn a_repeated_child_name_becomes_an_array_in_document_order() {
        assert_eq!(
            document_to_value(b"<a><item>1</item><item>2</item></a>").unwrap(),
            json!({ "a": { "item": ["1", "2"] } })
        );
    }

    #[test]
    fn namespace_prefixes_are_retained_on_element_and_attribute_keys() {
        assert_eq!(
            document_to_value(br#"<x:data xmlns:x="urn:e"><x:child x:id="7">v</x:child></x:data>"#).unwrap(),
            json!({ "x:data": { "@xmlns:x": "urn:e", "x:child": { "#text": "v", "@x:id": "7" } } })
        );
    }

    #[test]
    fn cdata_content_is_taken_verbatim() {
        assert_eq!(document_to_value(b"<x><![CDATA[a<b]]></x>").unwrap(), json!({ "x": "a<b" }));
    }

    #[test]
    fn mixed_content_keeps_both_text_and_child_keys() {
        assert_eq!(
            document_to_value(b"<p>Hello <b>x</b></p>").unwrap(),
            json!({ "p": { "#text": "Hello", "b": "x" } })
        );
    }

    #[test]
    fn a_processing_instruction_inside_an_element_becomes_a_pi_entry() {
        assert_eq!(
            document_to_value(br"<a><?target some data?></a>").unwrap(),
            json!({ "a": { "#pi": [ { "target": "target", "content": "some data" } ] } })
        );
    }

    #[test]
    fn a_comment_is_dropped() {
        assert_eq!(document_to_value(b"<a><!-- note --><b>1</b></a>").unwrap(), json!({ "a": { "b": "1" } }));
    }

    #[test]
    fn a_top_level_processing_instruction_is_attached_to_the_document() {
        assert_eq!(
            document_to_value(br#"<?xml-stylesheet href="x.xsl"?><root/>"#).unwrap(),
            json!({ "root": null, "#pi": [ { "target": "xml-stylesheet", "content": "href=\"x.xsl\"" } ] })
        );
    }

    #[test]
    fn predefined_and_numeric_entities_are_unescaped() {
        assert_eq!(document_to_value(b"<x>a &amp; b &#233;</x>").unwrap(), json!({ "x": "a & b \u{e9}" }));
    }

    #[test]
    fn an_unknown_entity_is_reported() {
        assert!(matches!(document_to_value(b"<x>&foo;</x>"), Err(XmlIngestError::UnknownEntity(_))));
    }

    #[test]
    fn a_declared_windows_1252_document_decodes_via_the_encoding_feature() {
        // 0xE9 is 'é' in windows-1252; the encoding feature must map it, not fail on non-UTF-8.
        let bytes = b"<?xml version=\"1.0\" encoding=\"windows-1252\"?><x>caf\xE9</x>";
        assert_eq!(document_to_value(bytes).unwrap(), json!({ "x": "caf\u{e9}" }));
    }

    #[test]
    fn bytes_invalid_for_the_declared_encoding_are_rejected_rather_than_replaced() {
        // 0xFF is not a valid standalone UTF-8 byte; strict decoding must reject it, never substitute
        // a U+FFFD replacement character.
        let bytes = b"<?xml version=\"1.0\" encoding=\"utf-8\"?><x>\xFF</x>";
        assert!(matches!(document_to_value(bytes), Err(XmlIngestError::Decode(_))));
    }

    #[test]
    fn an_input_without_a_root_element_is_reported_as_empty() {
        assert!(matches!(document_to_value(b"<!-- only a comment -->"), Err(XmlIngestError::EmptyDocument)));
    }
}
