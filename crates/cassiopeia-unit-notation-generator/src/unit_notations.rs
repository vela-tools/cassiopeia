use cefact_units::UnitCode;
use oxrdf::{Literal, NamedNodeRef, vocab::rdfs};
use std::collections::BTreeSet;
use unicode_normalization::UnicodeNormalization;

/// `qudt:uneceCommonCode`, the Recommendation No. 20 code of a unit.
const UNECE_COMMON_CODE: NamedNodeRef<'_> = NamedNodeRef::new_unchecked("http://qudt.org/schema/qudt/uneceCommonCode");
/// `qudt:symbol`, a unit's symbol.
const SYMBOL: NamedNodeRef<'_> = NamedNodeRef::new_unchecked("http://qudt.org/schema/qudt/symbol");
/// `qudt:altSymbol`, another symbol of a unit.
const ALT_SYMBOL: NamedNodeRef<'_> = NamedNodeRef::new_unchecked("http://qudt.org/schema/qudt/altSymbol");
/// `qudt:ucumCode`, a unit's case-sensitive UCUM code.
const UCUM_CODE: NamedNodeRef<'_> = NamedNodeRef::new_unchecked("http://qudt.org/schema/qudt/ucumCode");
/// `qudt:udunitsCode`, a unit's UDUNITS code.
const UDUNITS_CODE: NamedNodeRef<'_> = NamedNodeRef::new_unchecked("http://qudt.org/schema/qudt/udunitsCode");

/// Everything the vocabulary states about one unit that names it.
#[derive(Debug, Default)]
pub struct UnitNotations {
    /// The `qudt:uneceCommonCode` values Recommendation No. 20 lists, whatever their status.
    pub codes: BTreeSet<UnitCode>,
    /// The `qudt:uneceCommonCode` values no revision of Recommendation No. 20 lists.
    pub unknown_codes: BTreeSet<String>,
    /// The `qudt:symbol`, `qudt:altSymbol`, `qudt:ucumCode`, and `qudt:udunitsCode` values.
    pub symbols: BTreeSet<String>,
    /// The `rdfs:label` values in English or with no language.
    pub labels: BTreeSet<String>,
}

impl UnitNotations {
    /// Records `literal` as the value of `predicate`, when `predicate` is one the table reads.
    ///
    /// Symbols and labels are recorded composed to Unicode Normalization Form C. That changes no meaning,
    /// since the notations are compared composed anyway, and it keeps the table free of the decomposed
    /// spellings QUDT sometimes publishes, such as `Å` written as an `A` and a combining ring.
    pub fn record(&mut self, predicate: NamedNodeRef<'_>, literal: &Literal) {
        let value = literal.value();
        if predicate == UNECE_COMMON_CODE {
            if let Some(code) = UnitCode::from_code(value) {
                self.codes.insert(code);
            } else {
                self.unknown_codes.insert(value.to_owned());
            }
        } else if [SYMBOL, ALT_SYMBOL, UCUM_CODE, UDUNITS_CODE].contains(&predicate) {
            self.symbols.insert(value.nfc().collect());
        } else if predicate == rdfs::LABEL && literal.language().is_none_or(is_english) {
            self.labels.insert(value.nfc().collect());
        }
    }
}

/// Whether the BCP 47 tag `language` is English, of any region.
fn is_english(language: &str) -> bool {
    let primary = language.split('-').next().unwrap_or(language);
    primary.eq_ignore_ascii_case("en")
}

#[cfg(test)]
mod tests {
    use crate::unit_notations::{SYMBOL, UCUM_CODE, UNECE_COMMON_CODE, UnitNotations};
    use cefact_units::UnitCode;
    use oxrdf::{Literal, vocab::rdfs};
    use std::collections::BTreeSet;

    #[test]
    fn a_code_symbol_and_english_label_are_recorded() {
        let mut notations = UnitNotations::default();
        notations.record(UNECE_COMMON_CODE, &Literal::new_simple_literal("GQ"));
        notations.record(SYMBOL, &Literal::new_simple_literal("\u{3BC}g/m\u{B3}"));
        notations.record(UCUM_CODE, &Literal::new_simple_literal("ug.m-3"));
        notations.record(rdfs::LABEL, &Literal::new_language_tagged_literal_unchecked("Microgram per Cubic Metre", "en"));
        notations.record(
            rdfs::LABEL,
            &Literal::new_language_tagged_literal_unchecked("Microgram per Cubic Meter", "en-us"),
        );
        notations.record(rdfs::LABEL, &Literal::new_simple_literal("Microgram per Cubic Metre"));

        assert_eq!(notations.codes, BTreeSet::from([UnitCode::Gq]));
        assert_eq!(notations.symbols, BTreeSet::from(["ug.m-3".to_owned(), "\u{3BC}g/m\u{B3}".to_owned()]));
        assert_eq!(
            notations.labels,
            BTreeSet::from(["Microgram per Cubic Meter".to_owned(), "Microgram per Cubic Metre".to_owned()])
        );
    }

    #[test]
    fn a_decomposed_symbol_is_recorded_composed() {
        let mut notations = UnitNotations::default();
        notations.record(SYMBOL, &Literal::new_simple_literal("A\u{30A}"));

        assert_eq!(notations.symbols, BTreeSet::from(["\u{C5}".to_owned()]));
    }

    #[test]
    fn a_label_in_another_language_is_not_recorded() {
        let mut notations = UnitNotations::default();
        notations.record(rdfs::LABEL, &Literal::new_language_tagged_literal_unchecked("Mikrogramm pro Kubikmeter", "de"));

        assert!(notations.labels.is_empty());
    }

    #[test]
    fn a_code_no_revision_lists_is_kept_apart() {
        let mut notations = UnitNotations::default();
        notations.record(UNECE_COMMON_CODE, &Literal::new_simple_literal("H86"));

        assert!(notations.codes.is_empty());
        assert_eq!(notations.unknown_codes, BTreeSet::from(["H86".to_owned()]));
    }
}
