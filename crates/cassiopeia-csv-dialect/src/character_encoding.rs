use crate::error::DialectError;
use derive_more::Display;
use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};

/// The character encoding a CSV file is decoded from.
///
/// A manifest states it by any label of the WHATWG Encoding Standard, such as `latin1` or `utf-8`;
/// it is written back by the encoding's canonical name, so `latin1` reads back as `windows-1252`,
/// the encoding the standard maps that label to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, Serialize, Deserialize)]
#[display("{}", _0.name())]
#[serde(try_from = "String", into = "&'static str")]
pub struct CharacterEncoding(&'static Encoding);

impl CharacterEncoding {
    /// The encoding `encoding`.
    #[must_use]
    pub const fn new(encoding: &'static Encoding) -> CharacterEncoding {
        CharacterEncoding(encoding)
    }

    /// The encoding a decoder is built from.
    #[must_use]
    pub const fn encoding(self) -> &'static Encoding {
        self.0
    }
}

impl TryFrom<String> for CharacterEncoding {
    type Error = DialectError;

    fn try_from(label: String) -> Result<CharacterEncoding, DialectError> {
        Encoding::for_label(label.as_bytes())
            .map(CharacterEncoding)
            .ok_or(DialectError::UnknownEncoding(label))
    }
}

// Not derivable: an encoding is written back as its canonical name, which `encoding_rs` holds.
impl From<CharacterEncoding> for &'static str {
    fn from(encoding: CharacterEncoding) -> &'static str {
        encoding.0.name()
    }
}

#[cfg(test)]
mod tests {
    use crate::{character_encoding::CharacterEncoding, error::DialectError};
    use encoding_rs::{UTF_8, WINDOWS_1252};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_label_resolves_to_the_encoding_the_encoding_standard_maps_it_to() {
        assert_eq!(CharacterEncoding::try_from("latin1".to_owned()), Ok(CharacterEncoding::new(WINDOWS_1252)));
        assert_eq!(CharacterEncoding::try_from("utf8".to_owned()), Ok(CharacterEncoding::new(UTF_8)));
    }

    #[test]
    fn an_unknown_label_is_rejected() {
        assert_eq!(
            CharacterEncoding::try_from("klingon".to_owned()),
            Err(DialectError::UnknownEncoding("klingon".to_owned()))
        );
    }

    #[test]
    fn an_encoding_is_written_back_by_its_canonical_name() {
        let encoding: CharacterEncoding = serde_json::from_str(r#""latin1""#).unwrap();

        assert_eq!(serde_json::to_value(encoding).unwrap(), serde_json::json!("windows-1252"));
        assert_eq!(encoding.to_string(), "windows-1252");
    }
}
