pub(crate) mod geo_property;
pub(crate) mod identifier;
pub(crate) mod json_property;
pub(crate) mod language_property;
pub(crate) mod list_property;
pub(crate) mod property;
pub(crate) mod readers;
pub(crate) mod value_admission;
pub(crate) mod value_reading;
pub(crate) mod vocab_property;

#[cfg(test)]
pub(crate) mod fixtures;

use crate::attribute::{
    conversion::{readers::SourceParts, value_reading::ReadValue},
    refusal::AttributeRefusal,
};
use cassiopeia_geometry::policy::GeometryPolicy;
use cassiopeia_ir::attribute_content::AttributeContent;
use cassiopeia_mapping::conversion::Conversion;
use cassiopeia_ngsi_ld::value::omitted_nulls::OmittedNulls;
use serde_json::Value as JsonValue;

/// Converts the source values collected for one attribute into the content its type holds.
///
/// The conversion was resolved when the mapping loaded (see [`Conversion::resolve`]). Converting
/// runs in two steps: the conversion reads the parts, and the attribute's type admits what was
/// read. Each type admits in its own module, so each keeps or refuses a value by its own clause: a
/// `Property` and a `ListProperty` leave out every null, since none can sit in an NGSI-LD Value
/// (ETSI GS CIM 009 v1.9.1 clause 4.5.0), and count each in `omitted`; a `JsonProperty` keeps every
/// null, since its `json` member is never interpreted (clause 4.5.24.2); and a `GeoProperty`, a
/// `VocabProperty`, and a `LanguageProperty` hold only the geometry, IRI, or `languageMap` they read
/// (clauses 4.7.1, 4.5.20, and 4.5.18). An object nested `mappings` assemble converts as the one
/// part it is.
///
/// `Ok(None)` means the source carried no value.
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming what the attribute could not take.
pub(crate) fn apply(
    parts: SourceParts,
    conversion: Conversion,
    policy: Option<&GeometryPolicy>,
    omitted: &mut OmittedNulls,
) -> Result<Option<AttributeContent>, AttributeRefusal> {
    Ok(match conversion {
        Conversion::Property(conversion) => property::apply(parts, conversion, policy, omitted)?.map(AttributeContent::Value),
        Conversion::ListProperty(conversion) => list_property::apply(parts, conversion, policy, omitted)?.map(AttributeContent::Value),
        Conversion::JsonProperty(conversion) => json_property::apply(parts, conversion)?.map(AttributeContent::Json),
        Conversion::GeoProperty(target) => geo_property::apply(parts, target, policy, omitted)?.map(AttributeContent::Value),
        Conversion::LanguageProperty => language_property::apply(parts)?.map(AttributeContent::Language),
        Conversion::LanguageText => value_reading::read_text(parts, omitted).map(AttributeContent::Value),
        Conversion::VocabProperty(reading) => vocab_property::apply(parts, reading, omitted)?.map(AttributeContent::Vocab),
        // A relationship's object is built by the expander from its `source` and `target`, so
        // there is no value of its own to read here.
        Conversion::Relationship => None,
    })
}

/// Reads the source values collected for one member of an object nested `mappings` assemble, as
/// the raw JSON it contributes to that object.
///
/// A member's own type does not admit what it read: the assembled object is admitted once, by the
/// type of the attribute that holds it (see [`apply`]), which decides whether a null in it survives.
/// So the readings a `Property`, a `ListProperty`, and a `JsonProperty` keep as raw JSON stay raw,
/// nulls included, and every other member converts as it would on its own and is written as the
/// JSON its content stands for.
///
/// # Errors
/// Returns the [`AttributeRefusal`] naming what the member could not take.
pub(crate) fn read_member(
    parts: SourceParts,
    conversion: Conversion,
    policy: Option<&GeometryPolicy>,
    omitted: &mut OmittedNulls,
) -> Result<Option<JsonValue>, AttributeRefusal> {
    match conversion {
        Conversion::Property(conversion) => Ok(property::read(parts, conversion, policy, omitted)?.map(ReadValue::into_json)),
        Conversion::ListProperty(conversion) => list_property::read(parts, conversion, policy, omitted),
        Conversion::JsonProperty(conversion) => json_property::read(parts, conversion),
        Conversion::GeoProperty(_) | Conversion::LanguageProperty | Conversion::LanguageText | Conversion::VocabProperty(_) | Conversion::Relationship => {
            Ok(apply(parts, conversion, policy, omitted)?.map(JsonValue::from))
        }
    }
}
