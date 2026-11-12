use crate::{
    candidates::is_writable,
    notation_index::NotationIndex,
    notation_key::{NameKey, SymbolKey},
    qudt_unit_iri::QudtUnitId,
};
use cefact_units::UnitCode;

/// The UN/CEFACT Recommendation No. 20 code `notation` denotes, or `None` when it denotes none.
///
/// The notation is read, in this order, as:
///
/// 1. a QUDT unit IRI, `http` or `https`, such as `https://qudt.org/vocab/unit/MicroGM-PER-M3`;
/// 2. a symbol, UCUM code, or UDUNITS code, such as `µg/m³` or `ug/m3`, compared case-sensitively after
///    folding the characters that carry no meaning of their own, so the micro sign, the Greek mu, and
///    `u` meet, as do `m³`, `m^3`, and `m3`;
/// 3. a unit name, such as `microgram per cubic metre`, compared with ASCII case folded;
/// 4. a Recommendation No. 20 code itself, such as `GQ`, matched exactly.
///
/// Surrounding whitespace is ignored. A current code is preferred; a deprecated one is the result only for
/// a unit Recommendation No. 20 gives no current code, such as the hectare (`HAR`), and a deleted code is
/// never the result. A notation no source gives to exactly one unit, and that no ruling settles, resolves
/// to nothing rather than to a guess. So does a UCUM page anchor such as
/// `http://unitsofmeasure.org/ucum.html#para-30`, which names no unit.
#[must_use]
pub fn resolve(notation: &str) -> Option<UnitCode> {
    let index = NotationIndex::shared();
    let notation = notation.trim();

    QudtUnitId::from_iri(notation)
        .and_then(|id| index.qudt_unit(&id))
        .or_else(|| SymbolKey::of(notation).and_then(|key| index.symbol(&key)))
        .or_else(|| NameKey::of(notation).and_then(|key| index.name(&key)))
        .or_else(|| UnitCode::from_code(notation).filter(|&code| is_writable(code)))
}

#[cfg(test)]
mod tests {
    use crate::resolution::resolve;
    use cefact_units::UnitCode;
    use pretty_assertions::assert_eq;

    /// Asserts that each notation resolves as expected, naming the first that does not.
    macro_rules! assert_resolves {
        ($($notation:expr => $expected:expr),+ $(,)?) => {
            $(assert_eq!(resolve($notation), $expected, "resolving {:?}", $notation);)+
        };
    }

    #[test]
    fn the_notations_of_common_sensor_units_resolve() {
        assert_resolves! {
            "ug/m3" => Some(UnitCode::Gq),
            "ug.m-3" => Some(UnitCode::Gq),
            "mg/m3" => Some(UnitCode::Gp),
            "ppm" => Some(UnitCode::Code59),
            "[ppm]" => Some(UnitCode::Code59),
            "ppb" => Some(UnitCode::Code61),
            "\u{B0}C" => Some(UnitCode::Cel),
            "Cel" => Some(UnitCode::Cel),
            "degC" => Some(UnitCode::Cel),
            "K" => Some(UnitCode::Kel),
            "%" => Some(UnitCode::P1),
            "hPa" => Some(UnitCode::A97),
            "Pa" => Some(UnitCode::Pal),
            "dB" => Some(UnitCode::Code2n),
            "m/s" => Some(UnitCode::Mts),
            "m.s-1" => Some(UnitCode::Mts),
            "m s-1" => Some(UnitCode::Mts),
            "mm" => Some(UnitCode::Mmt),
            "m" => Some(UnitCode::Mtr),
            "1" => Some(UnitCode::C62),
            "{count}" => Some(UnitCode::C62),
        }
    }

    #[test]
    fn micro_sign_greek_mu_and_ascii_u_are_one_notation() {
        assert_resolves! {
            "\u{B5}g/m\u{B3}" => Some(UnitCode::Gq),
            "\u{3BC}g/m\u{B3}" => Some(UnitCode::Gq),
            "ug/m3" => Some(UnitCode::Gq),
            "\u{B5}g/m3" => Some(UnitCode::Gq),
            "ug/m^3" => Some(UnitCode::Gq),
        }
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert_resolves! { " ug/m3 " => Some(UnitCode::Gq) }
    }

    #[test]
    fn symbols_are_case_sensitive() {
        assert_resolves! {
            "MM" => None,
            "pa" => None,
            "Ppm" => None,
            "Mm" => Some(UnitCode::Mam),
        }
    }

    #[test]
    fn names_are_case_insensitive() {
        assert_resolves! {
            "Degree Celsius" => Some(UnitCode::Cel),
            "MICROGRAM PER CUBIC METRE" => Some(UnitCode::Gq),
            "microgram per cubic meter" => Some(UnitCode::Gq),
            "parts per million" => Some(UnitCode::Code59),
            "count" => Some(UnitCode::C62),
        }
    }

    #[test]
    fn qudt_iris_resolve_over_http_and_https_with_or_without_trailing_slash() {
        assert_resolves! {
            "http://qudt.org/vocab/unit/DEG_C" => Some(UnitCode::Cel),
            "https://qudt.org/vocab/unit/DEG_C/" => Some(UnitCode::Cel),
            "https://qudt.org/vocab/unit/MicroGM-PER-M3" => Some(UnitCode::Gq),
            "https://qudt.org/vocab/unit/NUM" => Some(UnitCode::C62),
        }
    }

    #[test]
    fn a_qudt_unit_id_is_case_sensitive() {
        assert_resolves! { "https://qudt.org/vocab/unit/deg_c" => None }
    }

    #[test]
    fn a_ucum_page_anchor_names_no_unit() {
        assert_resolves! { "http://unitsofmeasure.org/ucum.html#para-30" => None }
    }

    #[test]
    fn a_recommendation_20_code_resolves_only_as_spelled_and_never_when_deleted() {
        assert_resolves! {
            "KGM" => Some(UnitCode::Kgm),
            "kgm" => None,
            "HAR" => Some(UnitCode::Har),
            "GK" => None,
        }
    }

    #[test]
    fn an_unknown_notation_resolves_to_nothing() {
        assert_resolves! {
            "furlong per fortnight" => None,
            "" => None,
            "   " => None,
        }
    }

    #[test]
    fn a_weighted_decibel_reads_as_decibel() {
        assert_resolves! {
            "dB(A)" => Some(UnitCode::Code2n),
            "dBA" => Some(UnitCode::Code2n),
        }
    }

    #[test]
    fn a_qudt_unit_with_a_wrong_published_code_resolves_as_corrected() {
        assert_resolves! {
            "http://qudt.org/vocab/unit/BIT" => Some(UnitCode::A99),
            "http://qudt.org/vocab/unit/PERMILLE" => Some(UnitCode::Nx),
            "\u{2030}" => Some(UnitCode::Nx),
            "http://qudt.org/vocab/unit/PPT" => None,
        }
    }

    #[test]
    fn a_contested_symbol_resolves_as_ruled() {
        assert_resolves! {
            "V" => Some(UnitCode::Vlt),
            "rad" => Some(UnitCode::C81),
            "pH" => Some(UnitCode::Q30),
            "st" => None,
            "kt" => None,
        }
    }

    #[test]
    fn a_recommendation_20_unit_wins_over_a_package_of_the_same_name() {
        assert_resolves! { "piece" => Some(UnitCode::H87) }
    }

    #[test]
    fn a_unit_known_only_by_a_deprecated_code_resolves_to_it() {
        assert_resolves! {
            "http://qudt.org/vocab/unit/HA" => Some(UnitCode::Har),
            "hectare" => Some(UnitCode::Har),
            "mmHg" => Some(UnitCode::Hn),
            "Torr" => Some(UnitCode::Ua),
        }
    }

    #[test]
    fn a_current_code_wins_over_a_deprecated_one_for_the_same_unit() {
        assert_resolves! {
            "http://qudt.org/vocab/unit/QT_US" => Some(UnitCode::Qtl),
            "http://qudt.org/vocab/unit/KiloLB_F-PER-IN2" => Some(UnitCode::N20),
        }
    }
}
