use csv::Terminator;
use serde::{Deserialize, Serialize};

/// The line terminator a CSV file's records end with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineTerminator {
    /// A line terminated by `\n`.
    Lf,
    /// A line terminated by `\r\n`.
    Crlf,
}

impl LineTerminator {
    /// The terminator as the CSV reader takes it.
    #[must_use]
    pub const fn as_csv(self) -> Terminator {
        match self {
            LineTerminator::Lf => Terminator::Any(b'\n'),
            LineTerminator::Crlf => Terminator::CRLF,
        }
    }

    /// The terminator as the low-level CSV state machine takes it.
    pub(crate) const fn as_csv_core(self) -> csv_core::Terminator {
        match self {
            LineTerminator::Lf => csv_core::Terminator::Any(b'\n'),
            LineTerminator::Crlf => csv_core::Terminator::CRLF,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::line_terminator::LineTerminator;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_terminator_is_stated_in_kebab_case() {
        assert_eq!(serde_json::to_value(LineTerminator::Crlf).unwrap(), serde_json::json!("crlf"));
        assert_eq!(serde_json::from_str::<LineTerminator>(r#""lf""#).unwrap(), LineTerminator::Lf);
    }
}
