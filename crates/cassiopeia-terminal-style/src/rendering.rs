//! Whether output carries ANSI styling, and how that decision is made per output stream.

use std::{
    env::var_os,
    io::{IsTerminal, stderr, stdout},
};

/// Whether a rendered fragment carries ANSI styling or is emitted as bare text.
///
/// Colour is reserved for interactive terminals so that piped or captured output (bug reports,
/// scripts, redirection) stays plain and greppable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rendering {
    /// Emit ANSI escape codes.
    Colored,
    /// Emit bare, unstyled text.
    Plain,
}

/// The output stream a fragment is bound for.
///
/// The rendering decision gates on the interactivity of the *specific* stream the text is written
/// to: a report on stdout stays coloured only while stdout is a terminal, independently of whether
/// stderr happens to be redirected, and vice versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Colours output only for an interactive terminal on the named stream, honouring the `NO_COLOR`
/// convention.
#[must_use]
pub fn detect_rendering(stream: Stream) -> Rendering {
    let is_terminal = match stream {
        Stream::Stdout => stdout().is_terminal(),
        Stream::Stderr => stderr().is_terminal(),
    };
    if is_terminal && var_os("NO_COLOR").is_none() {
        Rendering::Colored
    } else {
        Rendering::Plain
    }
}

#[cfg(test)]
mod tests {
    use crate::rendering::{Rendering, Stream, detect_rendering};

    // Under the project's redirect-then-read test convention both standard streams are captured, so
    // neither is a terminal and the decision collapses to `Plain` regardless of `NO_COLOR`.
    #[test]
    fn stdout_is_plain_when_it_is_not_a_terminal() {
        assert_eq!(detect_rendering(Stream::Stdout), Rendering::Plain);
    }

    #[test]
    fn stderr_is_plain_when_it_is_not_a_terminal() {
        assert_eq!(detect_rendering(Stream::Stderr), Rendering::Plain);
    }
}
