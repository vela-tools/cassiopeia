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

/// Whether a stream is attached to an interactive terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interactivity {
    Terminal,
    Redirected,
}

/// Whether the environment asks for colour to be left out, per the `NO_COLOR` convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorPreference {
    Allowed,
    Refused,
}

/// Colours output only for an interactive terminal on the named stream, honouring the `NO_COLOR`
/// convention.
#[must_use]
pub fn detect_rendering(stream: Stream) -> Rendering {
    let is_terminal = match stream {
        Stream::Stdout => stdout().is_terminal(),
        Stream::Stderr => stderr().is_terminal(),
    };
    let interactivity = if is_terminal { Interactivity::Terminal } else { Interactivity::Redirected };
    let preference = if var_os("NO_COLOR").is_some() {
        ColorPreference::Refused
    } else {
        ColorPreference::Allowed
    };
    rendering_for(interactivity, preference)
}

/// The rendering a stream of the given interactivity gets under the given colour preference.
const fn rendering_for(interactivity: Interactivity, preference: ColorPreference) -> Rendering {
    match (interactivity, preference) {
        (Interactivity::Terminal, ColorPreference::Allowed) => Rendering::Colored,
        (Interactivity::Terminal, ColorPreference::Refused) | (Interactivity::Redirected, ColorPreference::Allowed | ColorPreference::Refused) => {
            Rendering::Plain
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rendering::{ColorPreference, Interactivity, Rendering, rendering_for};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_terminal_is_coloured_when_colour_is_allowed() {
        assert_eq!(rendering_for(Interactivity::Terminal, ColorPreference::Allowed), Rendering::Colored);
    }

    #[test]
    fn a_terminal_is_plain_when_no_color_refuses_colour() {
        assert_eq!(rendering_for(Interactivity::Terminal, ColorPreference::Refused), Rendering::Plain);
    }

    #[test]
    fn a_redirected_stream_is_plain_whatever_the_colour_preference() {
        assert_eq!(rendering_for(Interactivity::Redirected, ColorPreference::Allowed), Rendering::Plain);
        assert_eq!(rendering_for(Interactivity::Redirected, ColorPreference::Refused), Rendering::Plain);
    }
}
