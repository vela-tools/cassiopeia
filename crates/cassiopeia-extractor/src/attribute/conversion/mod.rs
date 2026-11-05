pub(crate) mod attribute_conversion;
pub(crate) mod geo_property;
pub(crate) mod identifier;
pub(crate) mod json_property;
pub(crate) mod language_property;
pub(crate) mod list_property;
pub(crate) mod property;
pub(crate) mod read_context;
pub(crate) mod readers;
pub(crate) mod value_admission;
pub(crate) mod value_read;
pub(crate) mod value_reading;
pub(crate) mod vocab_property;

#[cfg(test)]
pub(crate) mod fixtures;

use crate::attribute::{
    conversion::{
        attribute_conversion::ConversionOutput,
        language_property::{LanguageMapConversion, LanguageTextConversion},
        read_context::ReadContext,
        readers::SourceParts,
    },
    refusal::AttributeRefusal,
};
use cassiopeia_mapping::conversion::Conversion;

/// Converts the source values collected for one declaration into what it produces as `O`: an
/// attribute's own content, or the raw JSON a member of an object nested `mappings` assemble
/// contributes.
///
/// The conversion was resolved when the mapping loaded (see [`Conversion::resolve`]). Each type
/// converts in its own module, so each keeps or refuses a value by its own clause: a `Property` and
/// a `ListProperty` leave out every null, since none can sit in an NGSI-LD Value (ETSI GS CIM 009
/// v1.9.1 clause 4.5.0), and count each in the context; a `JsonProperty` keeps every null, since its
/// `json` member is never interpreted (clause 4.5.24.2); and a `GeoProperty`, a `VocabProperty`,
/// and a `LanguageProperty` hold only the geometry, IRI, or `languageMap` they read (clauses 4.7.1,
/// 4.5.20, and 4.5.18). An object nested `mappings` assemble converts as the one part it is.
///
/// `Ok(None)` means the source carried no value.
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming what the declaration could not take.
pub(crate) fn convert<O: ConversionOutput>(parts: SourceParts, conversion: Conversion, ctx: &mut ReadContext<'_>) -> Result<Option<O>, AttributeRefusal> {
    match conversion {
        Conversion::Property(conversion) => O::convert(conversion, parts, ctx),
        Conversion::ListProperty(conversion) => O::convert(conversion, parts, ctx),
        Conversion::JsonProperty(conversion) => O::convert(conversion, parts, ctx),
        Conversion::GeoProperty(target) => O::convert(target, parts, ctx),
        Conversion::LanguageProperty => O::convert(LanguageMapConversion, parts, ctx),
        Conversion::LanguageText => O::convert(LanguageTextConversion, parts, ctx),
        Conversion::VocabProperty(reading) => O::convert(reading, parts, ctx),
        // A relationship's object is built by the expander from its `source` and `target`, so
        // there is no value of its own to read here.
        Conversion::Relationship => Ok(None),
    }
}
