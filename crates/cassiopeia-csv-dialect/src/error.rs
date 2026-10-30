use thiserror::Error;

/// A stated CSV dialect member that cannot describe a file.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DialectError {
    /// A delimiter or quote that is not a single ASCII character, or that is a line break, which
    /// ends a record rather than structuring one.
    #[error("'{0}' is not a single ASCII character other than a line break")]
    InvalidByte(String),

    /// An encoding name that is not a label of the WHATWG Encoding Standard.
    #[error("'{0}' is not a known character encoding label")]
    UnknownEncoding(String),

    /// A null value token that is blank, which every blank field already reads as.
    #[error("A null value token must not be blank, since a blank field is already read as missing")]
    BlankNullToken,

    /// A stated delimiter and quote that are the same character, so no field could be quoted.
    #[error("The delimiter and the quote are both '{0}'")]
    DelimiterIsQuote(char),

    /// `header: false` stated together with a header of one or more records.
    #[error("'header' is false, but 'headerRows' is {0}")]
    HeaderAbsentWithRows(usize),

    /// `header: true` stated together with a header of no records.
    #[error("'header' is true, but 'headerRows' is 0")]
    HeaderPresentWithoutRows,
}

#[cfg(test)]
mod tests {
    use crate::error::DialectError;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_invalid_byte_names_the_rejected_text() {
        assert_eq!(
            DialectError::InvalidByte("ab".to_owned()).to_string(),
            "'ab' is not a single ASCII character other than a line break"
        );
    }

    #[test]
    fn a_header_contradiction_names_the_stated_row_count() {
        assert_eq!(DialectError::HeaderAbsentWithRows(3).to_string(), "'header' is false, but 'headerRows' is 3");
    }
}
