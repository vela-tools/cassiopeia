//! The single source of truth for the Cassiopeia constellation palette.
//!
//! Each constant is a semantic role, not a raw colour, so consumers speak in intent (`PRIMARY`,
//! `ACCENT`, `CAUTION`) and the mapping to Ansi256 codes lives in one place. The codes are drawn from
//! the stars of Cassiopeia and their surroundings: warm gold for Schedar (the K0-giant anchor),
//! steel blue for the hot B-type stars, peach for the nebula glow, slate and grey for the dimmer
//! companions and the interstellar dust between them.
//!
//! Consumers layer effects (`.effects(Effects::BOLD)`, an underline for a clap header) onto these
//! base roles as their own context requires.

use anstyle::{Ansi256Color, AnsiColor, Effects, Style};

/// The anchor of a report: bold and warm like Schedar (alpha Cas). Bold is baked in because every
/// use of this role (a filename, a version number, a clap usage line) wants it.
pub const PRIMARY: Style = Ansi256Color(178).on_default().effects(Effects::BOLD);

/// A structural accent (a format name, a heading, a provenance hash): steel blue, the hot B-type
/// stars of the constellation.
pub const ACCENT: Style = Ansi256Color(75).on_default();

/// A highlighted value: warm peach, the nebula glow reflecting Schedar.
pub const HIGHLIGHT: Style = Ansi256Color(216).on_default();

/// Subordinate detail (a size, a media type, a build environment): muted slate.
pub const MUTED: Style = Ansi256Color(246).on_default();

/// A field label or framing word: soft violet, distinct from the muted detail it introduces.
pub const LABEL: Style = Ansi256Color(140).on_default();

/// Separators and dim framing: grey, like the interstellar dust between the stars.
pub const FRAME: Style = Ansi256Color(241).on_default();

/// A successful or safe state: green, a safe approach vector.
pub const SUCCESS: Style = AnsiColor::Green.on_default();

/// A caution, not yet an error (a dirty tree, an absent capability, a warning): amber.
pub const CAUTION: Style = Ansi256Color(214).on_default();

/// An error: red, a red-shift warning.
pub const ERROR: Style = AnsiColor::Red.on_default();
