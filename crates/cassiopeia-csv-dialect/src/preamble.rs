use crate::record_end::skip_record;
use csv_core::Reader;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead};

/// The records above a CSV table that do not belong to it, such as a title line over the header.
///
/// The count is in logical CSV records, never physical lines. A quoted field may hold a line break
/// (RFC 4180 clause 2.6), so one preamble record can span several lines; counting records lets every
/// reader of a profiled file, which all parse through the same CSV state machine, skip exactly the
/// same bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Preamble(usize);

impl Preamble {
    /// No preamble: the table starts at the first record.
    pub const NONE: Preamble = Preamble(0);

    /// A preamble of `records` leading CSV records.
    #[must_use]
    pub const fn new(records: usize) -> Preamble {
        Preamble(records)
    }

    /// The number of leading CSV records the preamble covers.
    #[must_use]
    pub const fn records(self) -> usize {
        self.0
    }

    /// Advances `input` past the preamble's records, parsing them with `core` so a quoted line break
    /// stays inside its record. A preamble longer than the input consumes all of it.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`io::Error`] when `input` cannot be read.
    pub(crate) fn skip<R: BufRead>(self, core: &mut Reader, input: &mut R) -> io::Result<()> {
        (0..self.0).try_for_each(|_| skip_record(core, input).map(drop))
    }
}

#[cfg(test)]
mod tests {
    use crate::preamble::Preamble;
    use csv_core::ReaderBuilder;
    use pretty_assertions::assert_eq;
    use std::io::Read;

    fn remainder_after(preamble: Preamble, input: &[u8]) -> String {
        let mut input = input;
        preamble.skip(&mut ReaderBuilder::new().delimiter(b';').build(), &mut input).unwrap();
        let mut rest = String::new();
        input.read_to_string(&mut rest).unwrap();
        rest
    }

    #[test]
    fn no_preamble_consumes_nothing() {
        assert_eq!(remainder_after(Preamble::NONE, b"a;b\n1;2\n"), "a;b\n1;2\n");
    }

    #[test]
    fn a_one_record_preamble_consumes_exactly_the_first_line() {
        assert_eq!(remainder_after(Preamble::new(1), b"Lumes;v2.10\na;b\n1;2\n"), "a;b\n1;2\n");
    }

    #[test]
    fn a_quoted_line_break_keeps_a_two_line_preamble_record_whole() {
        let input = b"\"Lumes report\ngenerated 25.10.26\";v2.10\na;b\n1;2\n";

        assert_eq!(remainder_after(Preamble::new(1), input), "a;b\n1;2\n");
    }

    #[test]
    fn a_preamble_record_wider_than_the_scratch_space_is_skipped_whole() {
        let mut input = "x".repeat(4096);
        input.push_str(&";y".repeat(200));
        input.push_str("\na;b\n");

        assert_eq!(remainder_after(Preamble::new(1), input.as_bytes()), "a;b\n");
    }

    #[test]
    fn a_preamble_longer_than_the_input_consumes_all_of_it() {
        assert_eq!(remainder_after(Preamble::new(5), b"a;b\n1;2\n"), "");
    }

    #[test]
    fn a_preamble_serialises_as_its_record_count() {
        assert_eq!(serde_json::to_value(Preamble::new(2)).unwrap(), serde_json::json!(2));
    }
}
