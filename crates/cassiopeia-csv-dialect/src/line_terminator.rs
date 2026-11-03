use csv::Terminator;
use csv_core::Terminator as CoreTerminator;
use serde::{Deserialize, Serialize};

/// The line terminator a CSV file's records end with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineTerminator {
    /// A line terminated by `\n`.
    Lf,
    /// A line terminated by `\r\n`.
    Crlf,
    /// A line terminated by a lone `\r`, as classic Mac OS and some spreadsheet exports end lines.
    Cr,
}

impl LineTerminator {
    /// The terminator as the CSV reader takes it.
    #[must_use]
    pub const fn as_csv(self) -> Terminator {
        match self {
            LineTerminator::Lf => Terminator::Any(b'\n'),
            LineTerminator::Crlf => Terminator::CRLF,
            LineTerminator::Cr => Terminator::Any(b'\r'),
        }
    }

    /// The byte every line under this terminator ends with, so counting it counts the lines.
    #[must_use]
    pub const fn line_end(self) -> u8 {
        match self {
            LineTerminator::Lf | LineTerminator::Crlf => b'\n',
            LineTerminator::Cr => b'\r',
        }
    }

    /// The terminator as the low-level CSV state machine takes it.
    pub(crate) const fn as_csv_core(self) -> CoreTerminator {
        match self {
            LineTerminator::Lf => CoreTerminator::Any(b'\n'),
            LineTerminator::Crlf => CoreTerminator::CRLF,
            LineTerminator::Cr => CoreTerminator::Any(b'\r'),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::line_terminator::LineTerminator;
    use csv::ReaderBuilder;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_terminator_is_stated_in_kebab_case() {
        assert_eq!(serde_json::to_value(LineTerminator::Crlf).unwrap(), serde_json::json!("crlf"));
        assert_eq!(serde_json::from_str::<LineTerminator>(r#""lf""#).unwrap(), LineTerminator::Lf);
        assert_eq!(serde_json::from_str::<LineTerminator>(r#""cr""#).unwrap(), LineTerminator::Cr);
    }

    #[test]
    fn a_cr_terminator_splits_records_at_each_lone_carriage_return() {
        let mut reader = ReaderBuilder::new()
            .terminator(LineTerminator::Cr.as_csv())
            .from_reader("id,name\r1,Ada\r2,Grace\r".as_bytes());
        let names: Vec<String> = reader.records().map(|record| record.unwrap()[1].to_owned()).collect();

        assert_eq!(names, ["Ada", "Grace"]);
    }

    #[test]
    fn each_terminator_ends_its_lines_with_the_byte_it_is_counted_by() {
        assert_eq!(LineTerminator::Lf.line_end(), b'\n');
        assert_eq!(LineTerminator::Crlf.line_end(), b'\n');
        assert_eq!(LineTerminator::Cr.line_end(), b'\r');
    }
}
