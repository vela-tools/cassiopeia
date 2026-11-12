use crate::candidates::Candidates;
use cefact_units::UnitCode;
use std::hash::Hash;

/// Offers the key of each notation of `supplements` for its code.
pub(crate) fn offer_supplements<Key: Ord + Hash + Clone>(
    candidates: &mut Candidates<Key>,
    supplements: &[(&'static str, UnitCode)],
    key_of: fn(&'static str) -> Option<Key>,
) {
    for &(notation, code) in supplements {
        if let Some(key) = key_of(notation) {
            candidates.offer(key, code);
        }
    }
}

/// Symbols in common use that no source lists, with the code each denotes.
///
/// A test checks that no source lists any of them, so an entry a source comes to cover is removed
/// rather than kept beside it.
pub(crate) const SUPPLEMENTARY_SYMBOLS: [(&str, UnitCode); 5] = [
    // A-weighting is a frequency weighting applied to a level in decibels (IEC 61672-1), not a unit of
    // its own; a mapping carries the weighting in its attribute, as `NoiseLevelObserved`'s `LAeq` does.
    ("dB(A)", UnitCode::Code2n),
    ("dBA", UnitCode::Code2n),
    // UDUNITS-2 writes degree Celsius as `degC` and multiplies by juxtaposition, and the CF conventions
    // that NetCDF sources follow use both.
    ("degC", UnitCode::Cel),
    ("m s-1", UnitCode::Mts),
    // "An annotation without a leading symbol implies the default unit 1" (UCUM section 6), so `{count}`
    // alone is the unit one.
    ("{count}", UnitCode::C62),
];

/// Unit names in common use that no source lists, with the code each denotes.
pub(crate) const SUPPLEMENTARY_NAMES: [(&str, UnitCode); 1] = [
    // A plain count is a dimensionless quantity, whose unit Recommendation No. 20 names `one`.
    ("count", UnitCode::C62),
];

/// QUDT units that carry no `qudt:uneceCommonCode`, with the code each denotes.
pub(crate) const SUPPLEMENTARY_QUDT_UNITS: [(&str, UnitCode); 1] = [
    // QUDT gives `C62`, the unit of any dimensionless quantity, to `ONE` and `UNITLESS` but no code to
    // `NUM`, its unit of a plain number.
    ("NUM", UnitCode::C62),
];
