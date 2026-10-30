use csv_core::{ReadRecordResult, Reader};
use std::io::{self, BufRead};

/// Scratch bytes a skipped record's fields are parsed into and discarded from. A field longer than
/// this is parsed across several calls, so the size bounds memory rather than the record length.
const SCRATCH_FIELD_BYTES: usize = 1024;

/// Scratch field-end slots for a skipped record; a wider record is parsed across several calls.
const SCRATCH_FIELD_ENDS: usize = 64;

/// How a CSV record skipped from an input ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordEnd {
    /// The record's terminator was read, so the record is complete whatever follows it.
    Terminated,
    /// The input ended before a terminator: inside the record, such as within a quoted line break,
    /// or before any record at all.
    InputEnded,
}

/// Consumes one CSV record from `input`, parsing it with `core` so a quoted line break stays inside
/// its record, and discarding its fields.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] when `input` cannot be read.
pub(crate) fn skip_record<R: BufRead>(core: &mut Reader, input: &mut R) -> io::Result<RecordEnd> {
    let mut fields = [0u8; SCRATCH_FIELD_BYTES];
    let mut ends = [0usize; SCRATCH_FIELD_ENDS];
    loop {
        let buffer = input.fill_buf()?;
        // An empty buffer is how `csv_core` is told the input has ended, so a record it then
        // completes was cut off rather than terminated.
        let end = if buffer.is_empty() { RecordEnd::InputEnded } else { RecordEnd::Terminated };
        let (result, consumed, _, _) = core.read_record(buffer, &mut fields, &mut ends);
        input.consume(consumed);
        match result {
            ReadRecordResult::InputEmpty | ReadRecordResult::OutputFull | ReadRecordResult::OutputEndsFull => {}
            ReadRecordResult::Record => return Ok(end),
            ReadRecordResult::End => return Ok(RecordEnd::InputEnded),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::record_end::{RecordEnd, skip_record};
    use csv_core::{Reader, ReaderBuilder};
    use pretty_assertions::assert_eq;

    fn semicolon_core() -> Reader {
        ReaderBuilder::new().delimiter(b';').build()
    }

    #[test]
    fn a_record_ending_in_its_terminator_is_terminated() {
        let mut input = &b"a;b\n1;2\n"[..];

        assert_eq!(skip_record(&mut semicolon_core(), &mut input).unwrap(), RecordEnd::Terminated);
        assert_eq!(input, b"1;2\n");
    }

    #[test]
    fn a_last_record_without_a_terminator_ends_with_the_input() {
        let mut input = &b"1;2"[..];

        assert_eq!(skip_record(&mut semicolon_core(), &mut input).unwrap(), RecordEnd::InputEnded);
        assert_eq!(input, b"");
    }

    #[test]
    fn a_record_cut_inside_a_quoted_line_break_ends_with_the_input() {
        let mut input = &b"1;\"first line\n"[..];

        assert_eq!(skip_record(&mut semicolon_core(), &mut input).unwrap(), RecordEnd::InputEnded);
    }

    #[test]
    fn an_empty_input_holds_no_record() {
        let mut input = &b""[..];

        assert_eq!(skip_record(&mut semicolon_core(), &mut input).unwrap(), RecordEnd::InputEnded);
    }
}
