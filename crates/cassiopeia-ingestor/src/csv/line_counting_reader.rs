use encoding_rs_rw::MalformedError;
use std::io::{self, ErrorKind, Read};

/// A byte sequence that is not valid in the encoding a CSV file is read as, placed on the line of
/// the file it sits on.
#[derive(Debug, thiserror::Error)]
#[error("line {line} holds a byte sequence that is not valid in the file's encoding")]
pub struct MalformedLine {
    /// The one-based line of the file the malformed byte sequence sits on.
    pub line: usize,
    /// The decoder's report of the malformed byte sequence.
    #[source]
    pub source: MalformedError,
}

/// A reader over decoded UTF-8 that counts the lines it has passed on, so a malformed byte sequence
/// the decoder below it reports is placed on its line of the file.
///
/// Lines are counted in the decoded text, which ends every line with `\n` whatever the source
/// encoding, and from the very first byte of the file: a title above the table, the header, and a
/// quoted field spanning several lines all count.
pub struct LineCountingReader<R> {
    inner: R,
    newlines: usize,
}

impl<R: Read> LineCountingReader<R> {
    /// Counts the lines read through `inner`, a reader that decodes a file to UTF-8.
    pub const fn new(inner: R) -> LineCountingReader<R> {
        LineCountingReader { inner, newlines: 0 }
    }
}

impl<R: Read> Read for LineCountingReader<R> {
    /// Passes on what the decoder reads, turning its report of a malformed byte sequence into a
    /// [`MalformedLine`]. The decoder stops in front of the malformed bytes, so every line before
    /// them has been passed on and counted, and they sit on the line after the last newline.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(buf) {
            Ok(read) => {
                self.newlines += bytecount::count(&buf[..read], b'\n');
                Ok(read)
            }
            Err(error) => match error.downcast::<MalformedError>() {
                Ok(source) => Err(io::Error::new(
                    ErrorKind::InvalidData,
                    MalformedLine {
                        line: self.newlines + 1,
                        source,
                    },
                )),
                Err(error) => Err(error),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::csv::line_counting_reader::{LineCountingReader, MalformedLine};
    use encoding_rs::{UTF_8, WINDOWS_1253};
    use encoding_rs_rw::DecodingReader;
    use pretty_assertions::assert_eq;
    use std::io::{self, Read};

    fn read_all(reader: impl Read) -> io::Result<String> {
        let mut decoded = String::new();
        LineCountingReader::new(reader).read_to_string(&mut decoded).map(|_| decoded)
    }

    fn malformed_line(error: &io::Error) -> usize {
        error.get_ref().and_then(|inner| inner.downcast_ref::<MalformedLine>()).unwrap().line
    }

    #[test]
    fn well_formed_text_is_passed_on_unchanged() {
        let bytes = "id;name\n1;Café\n".as_bytes();

        assert_eq!(read_all(DecodingReader::new(bytes, UTF_8.new_decoder())).unwrap(), "id;name\n1;Café\n");
    }

    #[test]
    fn invalid_utf_8_is_placed_on_its_line() {
        let bytes = b"id;name\n1;Cafe\n2;M\xfcller\n";
        let error = read_all(DecodingReader::new(&bytes[..], UTF_8.new_decoder())).unwrap_err();

        assert_eq!(malformed_line(&error), 3);
    }

    #[test]
    fn an_unmapped_single_byte_is_placed_on_its_line() {
        // 0xD2 has no character in windows-1253.
        let bytes = b"id;name\n1;\xd2\n";
        let error = read_all(DecodingReader::new(&bytes[..], WINDOWS_1253.new_decoder())).unwrap_err();

        assert_eq!(malformed_line(&error), 2);
    }

    #[test]
    fn a_line_inside_a_quoted_field_is_counted() {
        let bytes = b"\"Lumes\nreport\";v2\nid;name\n1;\xff\n";
        let error = read_all(DecodingReader::new(&bytes[..], UTF_8.new_decoder())).unwrap_err();

        assert_eq!(malformed_line(&error), 4);
    }

    #[test]
    fn an_error_that_is_not_a_malformed_sequence_is_passed_on_as_it_is() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("disk gone"))
            }
        }
        let error = read_all(Failing).unwrap_err();

        assert_eq!(error.to_string(), "disk gone");
        assert!(error.get_ref().and_then(|inner| inner.downcast_ref::<MalformedLine>()).is_none());
    }
}
