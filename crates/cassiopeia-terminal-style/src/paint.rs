//! Applying a style to text, and joining painted fragments with the shared separator.

use crate::{palette::FRAME, rendering::Rendering};
use anstyle::Style;

/// The middot separator placed between the fragments of a banner or report line.
const SEPARATOR: &str = " \u{b7} ";

/// Wraps `text` in `style`'s ANSI codes when colouring, or returns it untouched when plain.
#[must_use]
pub fn paint(style: Style, text: &str, rendering: Rendering) -> String {
    match rendering {
        Rendering::Plain => text.to_owned(),
        Rendering::Colored => format!("{}{text}{}", style.render(), style.render_reset()),
    }
}

/// Joins already-painted `segments` with the shared middot separator, itself dimmed to [`FRAME`].
#[must_use]
pub fn join(segments: &[String], rendering: Rendering) -> String {
    segments.join(paint(FRAME, SEPARATOR, rendering).as_str())
}

#[cfg(test)]
mod tests {
    use crate::{
        paint::{join, paint},
        palette::FRAME,
        rendering::Rendering,
    };

    const ESCAPE: char = '\u{1b}';

    #[test]
    fn plain_paint_returns_the_text_unchanged() {
        assert_eq!(paint(FRAME, "grib", Rendering::Plain), "grib");
    }

    #[test]
    fn coloured_paint_wraps_the_text_in_escape_codes() {
        let painted = paint(FRAME, "grib", Rendering::Colored);

        assert!(painted.contains(ESCAPE));
        assert!(painted.contains("grib"));
    }

    #[test]
    fn join_places_the_middot_between_segments() {
        let joined = join(&["a".to_owned(), "b".to_owned()], Rendering::Plain);

        assert_eq!(joined, "a \u{b7} b");
    }

    #[test]
    fn join_of_a_single_segment_adds_no_separator() {
        assert_eq!(join(&["only".to_owned()], Rendering::Plain), "only");
    }
}
