//! The one definition of a field identifier in a template.
//!
//! An identifier is `_` or a Unicode `XID_Start` character, followed by any number of `XID_Continue`
//! characters: the default identifier syntax of Unicode Standard Annex #31, which Rust and Python use
//! for their own identifiers. Restricted to ASCII it is exactly Tera's identifier grammar,
//! `[A-Za-z_][A-Za-z0-9_]*`, so every name Tera reads as a variable is an identifier here too.
//! Beyond ASCII it admits names such as `čas`, `Città`, or `naïve` (in composed or decomposed form,
//! since `XID_Continue` covers combining marks) while still excluding whitespace, hyphens,
//! punctuation, and symbols, `²` among them. A general-category definition (`\p{L}` followed by
//! `\p{L}`/`\p{N}`) would reject a decomposed `č` and admit `²`, so it is not used.

/// Whether `character` can start an identifier.
pub(crate) fn is_identifier_start(character: char) -> bool {
    character == '_' || unicode_ident::is_xid_start(character)
}

/// Whether `character` can continue an identifier after its first character.
pub(crate) fn is_identifier_continue(character: char) -> bool {
    unicode_ident::is_xid_continue(character)
}

/// Whether `text` is exactly one identifier.
pub(crate) fn is_identifier(text: &str) -> bool {
    let mut characters = text.chars();

    characters.next().is_some_and(is_identifier_start) && characters.all(is_identifier_continue)
}

/// The byte length of the identifier `text` starts with, or zero when it does not start with one.
pub(crate) fn identifier_length(text: &str) -> usize {
    let mut characters = text.char_indices();
    match characters.next() {
        Some((_, first)) if is_identifier_start(first) => characters
            .find(|&(_, character)| !is_identifier_continue(character))
            .map_or(text.len(), |(end, _)| end),
        Some(_) | None => 0,
    }
}

#[cfg(test)]
mod tests {
    use crate::template::identifier::{identifier_length, is_identifier};

    #[test]
    fn ascii_identifiers_follow_teras_grammar() {
        for text in ["a", "_x", "_", "station_id", "x9", "A_B_9"] {
            assert!(is_identifier(text), "{text}");
        }
        for text in ["9a", "a-b", "a b", "a.b", "", "a$"] {
            assert!(!is_identifier(text), "{text}");
        }
    }

    #[test]
    fn unicode_letters_form_identifiers() {
        for text in ["čas", "Città", "številka", "naïve", "_ž", "Straße", "日付"] {
            assert!(is_identifier(text), "{text}");
        }
    }

    #[test]
    fn a_decomposed_letter_is_part_of_an_identifier() {
        assert!(is_identifier("c\u{30C}as"));
    }

    #[test]
    fn whitespace_punctuation_and_symbols_are_not_part_of_an_identifier() {
        for text in ["č as", "č-a", "a²", "a\u{a0}b", "€", "a→b"] {
            assert!(!is_identifier(text), "{text}");
        }
    }

    #[test]
    fn the_leading_identifier_length_is_measured_in_bytes() {
        assert_eq!(identifier_length("čas | upper"), "čas".len());
        assert_eq!(identifier_length("a-b"), 1);
        assert_eq!(identifier_length("naïve"), "naïve".len());
        assert_eq!(identifier_length("-a"), 0);
        assert_eq!(identifier_length(""), 0);
    }
}
