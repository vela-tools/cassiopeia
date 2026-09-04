use ::csv::Terminator;
use serde::Serialize;

/// Line terminators supported by the CSV profiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LineTerminator {
    /// A line terminated by `\n`.
    Lf,
    /// A line terminated by `\r\n`.
    CrLf,
}

impl LineTerminator {
    /// Converts the profiler representation to the CSV parser representation.
    #[must_use]
    pub const fn as_csv(self) -> Terminator {
        match self {
            Self::Lf => Terminator::Any(b'\n'),
            Self::CrLf => Terminator::CRLF,
        }
    }
}

/// A detected CSV dialect: the parameters needed to parse a specific CSV file.
#[derive(Debug, Clone, Serialize)]
pub struct Dialect {
    /// The field delimiter byte (comma, tab, semicolon, ...).
    pub delimiter: u8,
    /// The quote byte, or `None` when quoting is disabled.
    pub quote_char: Option<u8>,
    /// Whether the first row is a header row.
    pub has_headers: bool,
    /// The escape byte, if any.
    pub escape: Option<u8>,
    /// The line terminator.
    pub terminator: LineTerminator,
    /// The detected character encoding, as its canonical `encoding_rs` label (`UTF-8`,
    /// `windows-1252`, ...). The ingestor decodes the file with it so a non-UTF-8 source is read
    /// correctly rather than rejected on its first non-UTF-8 byte.
    pub encoding: String,
}
