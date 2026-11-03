//! The log writer that keeps every secret read from the environment out of a log sink.

use cassiopeia_common::environment::registry::mask;
use std::{
    borrow::Cow,
    io::{self, Write},
};
use tracing_subscriber::fmt::MakeWriter;

/// Makes writers that mask every secret this process has read before the bytes reach `inner`.
///
/// The fmt layer renders each event into one buffer and writes it whole, so a secret is never split
/// across two writes and is always seen entire.
pub struct Masking<M>(M);

impl<M> Masking<M> {
    /// Wraps the writer factory a log sink writes through.
    pub const fn new(inner: M) -> Masking<M> {
        Masking(inner)
    }
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for Masking<M> {
    type Writer = MaskingWriter<M::Writer>;

    fn make_writer(&'a self) -> MaskingWriter<M::Writer> {
        MaskingWriter(self.0.make_writer())
    }
}

/// A writer that masks every secret this process has read in what passes through it.
pub struct MaskingWriter<W>(W);

impl<W: Write> Write for MaskingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buffer);
        match mask(&text) {
            Cow::Borrowed(_) => self.0.write_all(buffer)?,
            Cow::Owned(masked) => self.0.write_all(masked.as_bytes())?,
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use crate::masking_writer::MaskingWriter;
    use cassiopeia_common::environment::{
        registry::register,
        secret::{Secret, Secrets},
        variable_name::VariableName,
    };
    use pretty_assertions::assert_eq;
    use secrecy::SecretString;
    use std::{io::Write, str::FromStr};

    #[test]
    fn a_registered_secret_never_reaches_the_sink() {
        let mut secrets = Secrets::new();
        secrets.insert(Secret::new(
            VariableName::from_str("LOG_SINK_TOKEN").unwrap(),
            &SecretString::from("log-sink-value-c41"),
        ));
        register(&secrets);
        let mut writer = MaskingWriter(Vec::new());

        writer.write_all(b"GET https://x/?t=log-sink-value-c41 failed\n").unwrap();

        assert_eq!(String::from_utf8(writer.0).unwrap(), "GET https://x/?t=${LOG_SINK_TOKEN} failed\n");
    }

    #[test]
    fn a_line_without_a_secret_passes_through_unchanged() {
        let mut writer = MaskingWriter(Vec::new());

        writer.write_all(b"plain line\n").unwrap();

        assert_eq!(writer.0, b"plain line\n");
    }
}
