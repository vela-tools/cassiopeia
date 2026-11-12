use crate::qudt::qudt_unit::QudtUnit;
use cefact_units::UnitCode;

/// A QUDT unit whose published `qudt:uneceCommonCode` denotes a different unit.
#[derive(Debug)]
pub(crate) struct QudtCorrection {
    /// The unit, by its local name in the vocabulary.
    pub(crate) unit: &'static str,
    /// The code QUDT publishes for it. The correction applies only while QUDT still publishes this code
    /// alone, so it lapses once QUDT fixes the unit.
    pub(crate) published: UnitCode,
    /// The code that denotes the unit, or `None` when no Recommendation No. 20 code does.
    pub(crate) corrected: Option<UnitCode>,
}

/// Every QUDT unit Cassiopeia reads with a code other than the one QUDT publishes, found by comparing
/// each unit's English label and conversion multiplier with the name and conversion factor
/// Recommendation No. 20 gives its published code.
///
/// The correction applies to everything the unit carries: its IRI, its symbols, and its labels.
pub(crate) static QUDT_CORRECTIONS: [QudtCorrection; 28] = [
    // Barrel (US petroleum) per second, published as per hour.
    corrected("BBL_US_PET-PER-SEC", UnitCode::J62, UnitCode::J63),
    // The bit, published as barrel (US petroleum) per second.
    corrected("BIT", UnitCode::J63, UnitCode::A99),
    // The logarithmic decade of a frequency interval, published as the ten-year decade.
    corrected("DECADE", UnitCode::Dec, UnitCode::P41),
    // Brake horsepower, 745.7 W, published as boiler horsepower, 9809.5 W.
    corrected("HP_Brake", UnitCode::K42, UnitCode::Bhp),
    // Kilopound-force per square inch, published as the deprecated `84`, which Recommendation No. 20 itself
    // says to replace with kip per square inch, `N20`.
    corrected("KiloLB_F-PER-IN2", UnitCode::Code84, UnitCode::N20),
    // Megahertz metre, published as megahertz kilometre.
    corrected("MegaHZ-M", UnitCode::H39, UnitCode::M27),
    // Millilitre per square centimetre minute, published as per second.
    corrected("MilliL-PER-CentiM2-MIN", UnitCode::Code35, UnitCode::M22),
    // Percent per ten thousand, published as per one hundred thousand.
    corrected("PERCENT-PER-DecaKiloCOUNT", UnitCode::H92, UnitCode::H91),
    // The per mille, published as degree API.
    corrected("PERMILLE", UnitCode::J13, UnitCode::Nx),
    // The short ton, 907.18474 kg, published as the tonne.
    corrected("TON", UnitCode::Tne, UnitCode::Stn),
    // Btu(IT) per pound-force degree, published as per pound (mass) degree.
    withdrawn("BTU_IT-PER-LB_F-DEG_F", UnitCode::J43),
    withdrawn("BTU_IT-PER-LB_F-DEG_R", UnitCode::A21),
    // Decibel milliwatt per megahertz, a spectral density, published as the decibel milliwatt.
    withdrawn("DeciB-MilliW-PER-MegaHZ", UnitCode::Dbm),
    // The decisecond, published as the millisecond.
    withdrawn("DeciSEC", UnitCode::C26),
    // Grain per imperial gallon, published as per US gallon.
    withdrawn("GRAIN-PER-GAL_IMP", UnitCode::K41),
    // Gross tonnage, an index of a ship's volume, published as the gross ton, a mass of 2240 pounds.
    withdrawn("GT", UnitCode::Gt),
    // Gigajoule and megajoule per hour, published as joule per hour.
    withdrawn("GigaJ-PER-HR", UnitCode::P16),
    withdrawn("MegaJ-PER-HR", UnitCode::P16),
    // Mechanical horsepower, 745.7 W, published as electric horsepower, 746 W.
    withdrawn("HP", UnitCode::K43),
    // Kilopound-force units, published as their pound-force counterparts.
    withdrawn("KiloLB_F-FT-PER-A", UnitCode::F22),
    withdrawn("KiloLB_F-FT-PER-LB", UnitCode::G20),
    withdrawn("KiloLB_F-PER-FT", UnitCode::F17),
    // QUDT's month is the synodic month of 29.53 days; Recommendation No. 20's is a twelfth of a year.
    withdrawn("MO", UnitCode::Mon),
    withdrawn("PER-MO", UnitCode::H11),
    withdrawn("PERCENT-PER-MO", UnitCode::H71),
    withdrawn("TONNE-PER-MO", UnitCode::M88),
    // Ounce-force per cubic inch, published as ounce (mass) per cubic inch.
    withdrawn("OZ_F-PER-IN3", UnitCode::L39),
    // Part per trillion, published as part per thousand.
    withdrawn("PPT", UnitCode::Nx),
];

/// A correction reading `unit` as `reading` where QUDT publishes `published`.
const fn corrected(unit: &'static str, published: UnitCode, reading: UnitCode) -> QudtCorrection {
    QudtCorrection {
        unit,
        published,
        corrected: Some(reading),
    }
}

/// A correction reading `unit` as no code where QUDT publishes `published`.
const fn withdrawn(unit: &'static str, published: UnitCode) -> QudtCorrection {
    QudtCorrection {
        unit,
        published,
        corrected: None,
    }
}

/// The codes Cassiopeia reads `unit` as: its correction's code when a correction applies to it, else the
/// codes QUDT publishes.
pub(crate) fn codes_read(unit: &QudtUnit) -> &'static [UnitCode] {
    QUDT_CORRECTIONS
        .iter()
        .find(|correction| correction.unit == unit.id && unit.codes == [correction.published])
        .map_or(unit.codes, |correction| correction.corrected.as_slice())
}

#[cfg(test)]
mod tests {
    use crate::qudt::{
        correction::{QUDT_CORRECTIONS, codes_read},
        generated::QUDT_UNITS,
    };
    use cefact_units::{Status, UnitCode};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;

    #[test]
    fn every_correction_contradicts_the_code_qudt_still_publishes() {
        for correction in &QUDT_CORRECTIONS {
            let unit = QUDT_UNITS.iter().find(|unit| unit.id == correction.unit);
            assert_eq!(
                unit.map(|unit| unit.codes),
                Some([correction.published].as_slice()),
                "the correction of {} no longer matches QUDT",
                correction.unit
            );
        }
    }

    #[test]
    fn every_corrected_code_is_current_and_differs_from_the_published_one() {
        for correction in &QUDT_CORRECTIONS {
            if let Some(code) = correction.corrected {
                assert_eq!(code.status(), Status::Current, "{}", correction.unit);
                assert_ne!(code, correction.published, "{}", correction.unit);
            }
        }
    }

    #[test]
    fn no_unit_is_corrected_twice() {
        let units: BTreeSet<&str> = QUDT_CORRECTIONS.iter().map(|correction| correction.unit).collect();

        assert_eq!(units.len(), QUDT_CORRECTIONS.len());
    }

    #[test]
    fn a_corrected_unit_reads_as_its_correction_and_others_as_published() {
        let unit = |id: &str| QUDT_UNITS.iter().find(|unit| unit.id == id).unwrap();

        assert_eq!(codes_read(unit("BIT")), [UnitCode::A99]);
        assert_eq!(codes_read(unit("PPT")), []);
        assert_eq!(codes_read(unit("MicroGM-PER-M3")), [UnitCode::Gq]);
    }
}
