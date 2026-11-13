use crate::entity_id::EntityId;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

/// The bytes a text key is percent-encoded in: those the WHATWG URL Standard's path percent-encode
/// set holds, which a path cannot hold literally, `/` and `\`, which would end the path segment, and
/// `%`, which would start an escape. Every other ASCII byte, `'` and `OData`'s parentheses among them,
/// is kept, and every non-ASCII byte is encoded.
const KEY_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'\\')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// The key that addresses the entity `id` in a path, as in `Datastreams(56776)` (OGC 18-088 section
/// 9.2.3): `(56776)` for an integer id, and for a text id the text as an `OData` string literal, in
/// single quotes with each `'` doubled (section 9.3.3.5, Req 29), percent-encoded where a path
/// segment cannot hold a byte, so `o'neil` is `('o''neil')`.
#[must_use]
pub fn entity_key(id: &EntityId) -> String {
    match id {
        EntityId::Integer(id) => format!("({id})"),
        EntityId::Text(id) => format!("('{}')", utf8_percent_encode(&id.replace('\'', "''"), KEY_SEGMENT)),
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_id::EntityId, request::entity_key::entity_key};
    use pretty_assertions::assert_eq;
    use url::Url;

    fn text(id: &str) -> EntityId {
        EntityId::Text(id.into())
    }

    #[test]
    fn entity_ids_are_written_as_path_segments() {
        assert_eq!(entity_key(&EntityId::Integer(56776)), "(56776)");
        assert_eq!(entity_key(&text("abc")), "('abc')");
        assert_eq!(entity_key(&text("o'neil")), "('o''neil')");
        assert_eq!(entity_key(&text("a b")), "('a%20b')");
    }

    #[test]
    fn a_text_key_never_ends_its_segment_or_starts_a_query() {
        assert_eq!(entity_key(&text("a/b?c#d%e\\f")), "('a%2Fb%3Fc%23d%25e%5Cf')");
        assert_eq!(entity_key(&text("µ")), "('%C2%B5')");
    }

    #[test]
    fn a_key_survives_url_parsing_unchanged() {
        let key = entity_key(&text("o'neil (2) \"x\" <y> {z} ^`"));

        let url = Url::parse(&format!("https://h.example/v1.1/Datastreams{key}/Observations")).unwrap();

        assert_eq!(url.path(), format!("/v1.1/Datastreams{key}/Observations"));
    }
}
