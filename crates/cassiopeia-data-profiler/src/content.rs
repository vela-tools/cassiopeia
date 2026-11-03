use cassiopeia_encoding_detection::encoding_hint::EncodingHint;

/// The bytes being profiled, together with what their source says about their encoding.
#[derive(Debug, Clone, Copy)]
pub struct Content<'a> {
    /// The full content of the file or buffer.
    bytes: &'a [u8],
    /// The encoding context the source supplied, if any.
    encoding_hint: &'a EncodingHint,
}

impl<'a> Content<'a> {
    /// Content whose source supplied no encoding context, such as a local file.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Content<'a> {
        Content {
            bytes,
            encoding_hint: &EncodingHint::NONE,
        }
    }

    /// Content carrying the encoding context its source supplied.
    #[must_use]
    pub const fn hinted(bytes: &'a [u8], encoding_hint: &'a EncodingHint) -> Content<'a> {
        Content { bytes, encoding_hint }
    }

    /// The full content.
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The encoding context the source supplied.
    #[must_use]
    pub const fn encoding_hint(&self) -> &'a EncodingHint {
        self.encoding_hint
    }
}

#[cfg(test)]
mod tests {
    use crate::content::Content;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use mediatype::MediaTypeBuf;
    use pretty_assertions::assert_eq;
    use url::Url;

    #[test]
    fn content_without_a_hint_carries_its_bytes_and_no_encoding_context() {
        let content = Content::new(b"id;name\n");

        assert_eq!(content.bytes(), b"id;name\n");
        assert_eq!(content.encoding_hint(), &EncodingHint::NONE);
    }

    #[test]
    fn hinted_content_carries_the_hint_its_source_supplied() {
        let content_type = "text/csv; charset=windows-1252".parse::<MediaTypeBuf>().unwrap();
        let hint = EncodingHint::for_remote_source(&Url::parse("https://www.wien.gv.at/feed.csv").unwrap(), Some(&content_type));
        let content = Content::hinted(b"id;name\n", &hint);

        assert_eq!(content.bytes(), b"id;name\n");
        assert_eq!(content.encoding_hint(), &hint);
    }
}
