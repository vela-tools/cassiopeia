use unicode_normalization::UnicodeNormalization;

/// A symbol, UCUM code, or UDUNITS code in the form notations of those kinds are compared in.
///
/// Two spellings of one symbol often differ only in characters that carry no meaning of their own: the
/// micro sign (U+00B5) against the Greek small letter mu (U+03BC) and the ASCII `u` that UCUM writes for
/// both, `³` against `3` or `^3`, a middle dot (U+00B7) against the `.` that UCUM multiplies with, a
/// precomposed `Å` against an `A` followed by a combining ring. The key composes the notation to Unicode
/// Normalization Form C, folds each such character to one form, collapses every run of whitespace to one
/// space, and trims the ends. Case is kept: UCUM codes are case-sensitive, and `Mm` (megametre) is not
/// `mm` (millimetre).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct SymbolKey(String);

impl SymbolKey {
    /// The key `notation` is compared under, or `None` when nothing of it remains to compare.
    pub(crate) fn of(notation: &str) -> Option<SymbolKey> {
        non_empty(folded(notation)).map(SymbolKey)
    }
}

/// A unit name in the form names are compared in.
///
/// The name is folded as a [`SymbolKey`] folds a symbol, and its ASCII letters are then lowered, so
/// `Degree Celsius` and `degree celsius` are one name. Letters outside ASCII keep their case.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct NameKey(String);

impl NameKey {
    /// The key `notation` is compared under, or `None` when nothing of it remains to compare.
    pub(crate) fn of(notation: &str) -> Option<NameKey> {
        let mut name = folded(notation);
        name.make_ascii_lowercase();
        non_empty(name).map(NameKey)
    }
}

/// `text` unless it is empty.
fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// `notation` composed to Normalization Form C, with each run of whitespace collapsed to one space, the
/// ends trimmed, and every character folded by [`push_folded`].
///
/// Composition settles what Unicode itself calls equivalent, the Kelvin sign and `K` among it, so
/// [`push_folded`] only has to settle what Unicode keeps apart.
fn folded(notation: &str) -> String {
    let composed: String = notation.nfc().collect();
    let mut folded = String::with_capacity(composed.len());
    for (position, word) in composed.split_whitespace().enumerate() {
        if position > 0 {
            folded.push(' ');
        }
        word.chars().for_each(|character| push_folded(&mut folded, character));
    }
    folded
}

/// Appends the form `character` is compared in.
///
/// Letter-like symbols fold to the letters they stand for, superscript digits and signs to their ASCII
/// forms, and the caret UCUM's print form writes before an exponent (`m^3`) is dropped, so `m³`, `m^3`,
/// and `m3` meet.
fn push_folded(into: &mut String, character: char) {
    match character {
        // Micro sign and Greek small letter mu: UCUM writes the micro prefix as `u`.
        '\u{B5}' | '\u{3BC}' => into.push('u'),
        // Degree Celsius sign and degree Fahrenheit sign.
        '\u{2103}' => into.push_str("\u{B0}C"),
        '\u{2109}' => into.push_str("\u{B0}F"),
        // Masculine ordinal indicator and ring above: Recommendation No. 20 writes `ºF` where it means `°F`.
        '\u{BA}' | '\u{2DA}' => into.push('\u{B0}'),
        // Middle dot and dot operator: UCUM multiplies with `.`.
        '\u{B7}' | '\u{22C5}' => into.push('.'),
        '\u{2070}' => into.push('0'),
        '\u{B9}' => into.push('1'),
        '\u{B2}' => into.push('2'),
        '\u{B3}' => into.push('3'),
        '\u{2074}' => into.push('4'),
        '\u{2075}' => into.push('5'),
        '\u{2076}' => into.push('6'),
        '\u{2077}' => into.push('7'),
        '\u{2078}' => into.push('8'),
        '\u{2079}' => into.push('9'),
        '\u{207A}' => into.push('+'),
        '\u{207B}' => into.push('-'),
        '^' => {}
        other => into.push(other),
    }
}

#[cfg(test)]
mod tests {
    use crate::notation_key::{NameKey, SymbolKey};
    use pretty_assertions::assert_eq;

    fn symbol(notation: &str) -> Option<String> {
        SymbolKey::of(notation).map(|key| key.0)
    }

    fn name(notation: &str) -> Option<String> {
        NameKey::of(notation).map(|key| key.0)
    }

    #[test]
    fn the_micro_sign_greek_mu_and_ascii_u_fold_to_one_symbol() {
        assert_eq!(symbol("\u{B5}g/m\u{B3}"), Some("ug/m3".to_owned()));
        assert_eq!(symbol("\u{3BC}g/m\u{B3}"), Some("ug/m3".to_owned()));
        assert_eq!(symbol("ug/m^3"), Some("ug/m3".to_owned()));
    }

    #[test]
    fn superscript_exponents_fold_to_ascii() {
        assert_eq!(symbol("m\u{207B}\u{B9}"), Some("m-1".to_owned()));
        assert_eq!(symbol("W/(m\u{B2}\u{B7}K\u{2074})"), Some("W/(m2.K4)".to_owned()));
    }

    #[test]
    fn letter_like_symbols_fold_to_the_letters_they_stand_for() {
        assert_eq!(symbol("\u{2103}"), Some("\u{B0}C".to_owned()));
        assert_eq!(symbol("\u{BA}F"), Some("\u{B0}F".to_owned()));
        assert_eq!(symbol("\u{212A}"), Some("K".to_owned()));
        assert_eq!(symbol("\u{2126}"), Some("\u{3A9}".to_owned()));
    }

    #[test]
    fn canonically_equivalent_spellings_are_one_symbol() {
        assert_eq!(symbol("A\u{30A}"), Some("\u{C5}".to_owned()));
        assert_eq!(symbol("\u{212B}"), Some("\u{C5}".to_owned()));
    }

    #[test]
    fn a_middle_dot_folds_to_the_ucum_multiplication_dot() {
        assert_eq!(symbol("N\u{B7}m"), Some("N.m".to_owned()));
        assert_eq!(symbol("N\u{22C5}m"), Some("N.m".to_owned()));
    }

    #[test]
    fn whitespace_is_trimmed_and_collapsed() {
        assert_eq!(symbol(" ug/m3 "), Some("ug/m3".to_owned()));
        assert_eq!(symbol("fl\u{A0} oz"), Some("fl oz".to_owned()));
    }

    #[test]
    fn a_symbol_keeps_its_case() {
        assert_eq!(symbol("Mm"), Some("Mm".to_owned()));
        assert_eq!(symbol("mm"), Some("mm".to_owned()));
    }

    #[test]
    fn a_name_lowers_ascii_letters_only() {
        assert_eq!(name("Degree Celsius"), Some("degree celsius".to_owned()));
        assert_eq!(name("\u{C5}ngstr\u{F6}m"), Some("\u{C5}ngstr\u{F6}m".to_owned()));
    }

    #[test]
    fn an_empty_or_blank_notation_has_no_key() {
        assert_eq!(symbol(""), None);
        assert_eq!(symbol(" \t "), None);
        assert_eq!(name(""), None);
    }
}
