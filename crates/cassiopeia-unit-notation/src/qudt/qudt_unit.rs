use cefact_units::UnitCode;

/// A unit of the QUDT unit vocabulary that names a Recommendation No. 20 code, with every notation QUDT
/// gives it.
///
/// The values are QUDT's as published, with symbols and labels composed to Unicode Normalization Form C;
/// `cassiopeia-unit-notation-generator` writes them to [`QUDT_UNITS`](crate::qudt::generated::QUDT_UNITS)
/// and changes nothing else. Where a published code is wrong,
/// [`codes_read`](crate::qudt::correction::codes_read) says what Cassiopeia reads instead.
#[derive(Debug)]
pub(crate) struct QudtUnit {
    /// The unit's local name in the vocabulary, as in `http://qudt.org/vocab/unit/MicroGM-PER-M3`.
    pub(crate) id: &'static str,
    /// Its `qudt:uneceCommonCode` values, whatever their status.
    pub(crate) codes: &'static [UnitCode],
    /// Its `qudt:symbol`, `qudt:altSymbol`, `qudt:ucumCode`, and `qudt:udunitsCode` values.
    pub(crate) symbols: &'static [&'static str],
    /// Its `rdfs:label` values in English or with no language.
    pub(crate) labels: &'static [&'static str],
}
