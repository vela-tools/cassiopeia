//! A `label value` fragment: a violet label introducing a highlighted value.

use crate::{
    paint::paint,
    palette::{HIGHLIGHT, LABEL},
    rendering::Rendering,
};

/// Builds a `label value` fragment, the label in [`LABEL`] violet and the value in [`HIGHLIGHT`]
/// peach, with a single space between them.
#[must_use]
pub fn field(label: &str, value: &str, rendering: Rendering) -> String {
    format!("{}{}", paint(LABEL, &format!("{label} "), rendering), paint(HIGHLIGHT, value, rendering))
}

#[cfg(test)]
mod tests {
    use crate::{field::field, rendering::Rendering};

    const ESCAPE: char = '\u{1b}';

    #[test]
    fn a_plain_field_joins_the_label_and_value_with_a_space() {
        assert_eq!(field("edition", "2", Rendering::Plain), "edition 2");
    }

    #[test]
    fn a_coloured_field_carries_escape_codes_around_both_parts() {
        assert!(field("edition", "2", Rendering::Colored).contains(ESCAPE));
    }
}
