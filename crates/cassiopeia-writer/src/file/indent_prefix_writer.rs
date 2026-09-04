use memchr::memchr;
use std::io::{self, Write};

/// A pass-through [`Write`] adapter that injects a fixed byte sequence after every `\n` it sees.
///
/// [`EntityTypeSink`](crate::file::entity_type_sink::EntityTypeSink) uses it to nest each entity one
/// indent level deeper than the surrounding array without materializing the entity's pretty-printed
/// form into an intermediate `String`. `memchr` gives a SIMD-accelerated newline scan, so the pass
/// is effectively a memcpy with periodic `write_all` of the indent prefix.
pub struct IndentPrefixWriter<'a, W: Write> {
    /// The underlying writer the indented bytes flow into.
    pub inner: &'a mut W,
    /// The prefix inserted after each newline.
    pub indent: &'static [u8],
}

impl<W: Write> Write for IndentPrefixWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut pos = 0;
        while pos < buf.len() {
            let Some(rel) = memchr(b'\n', &buf[pos..]) else {
                self.inner.write_all(&buf[pos..])?;
                break;
            };

            let end = pos + rel + 1;
            self.inner.write_all(&buf[pos..end])?;
            self.inner.write_all(self.indent)?;
            pos = end;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use crate::file::indent_prefix_writer::IndentPrefixWriter;
    use std::io::Write;

    #[test]
    fn a_single_write_prefixes_every_newline() {
        let pretty = "{\n  \"a\": 1,\n  \"b\": [\n    2,\n    3\n  ]\n}";
        let expected = pretty.replace('\n', "\n  ");

        let mut sink: Vec<u8> = Vec::new();
        {
            let mut writer = IndentPrefixWriter {
                inner: &mut sink,
                indent: b"  ",
            };
            writer.write_all(pretty.as_bytes()).unwrap();
        }

        assert_eq!(sink, expected.as_bytes());
    }

    #[test]
    fn writes_split_across_chunks_produce_the_same_bytes() {
        let pretty = "{\n  \"a\": 1,\n  \"b\": 2\n}";
        let expected = pretty.replace('\n', "\n  ");

        let mut sink: Vec<u8> = Vec::new();
        {
            let mut writer = IndentPrefixWriter {
                inner: &mut sink,
                indent: b"  ",
            };
            writer.write_all(&pretty.as_bytes()[..5]).unwrap();
            writer.write_all(&pretty.as_bytes()[5..12]).unwrap();
            writer.write_all(&pretty.as_bytes()[12..]).unwrap();
        }

        assert_eq!(sink, expected.as_bytes());
    }
}
