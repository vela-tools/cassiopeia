use crate::encoding_hint::EncodingHint;

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
