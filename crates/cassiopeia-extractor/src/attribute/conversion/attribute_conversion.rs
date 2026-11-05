use crate::attribute::{
    conversion::{read_context::ReadContext, readers::SourceParts},
    refusal::AttributeRefusal,
};
use cassiopeia_ir::attribute_content::AttributeContent;
use cassiopeia_ngsi_ld::value::omitted_nulls::OmittedNulls;
use serde_json::Value as JsonValue;

/// How one attribute type turns the source values collected for a declaration into the content it
/// holds, in two steps: the conversion reads the parts, and the type admits what was read.
///
/// Reading never leaves out a null that raw JSON holds; only admission into a type whose value
/// cannot hold one does. So what was read is also what a member of an object nested `mappings`
/// assemble contributes, since that object is admitted once, by the type of the attribute holding
/// it.
pub(crate) trait AttributeConversion: Copy {
    /// What reading the parts produces, before the type admits it.
    type Reading;
    /// What the type holds once it admits a reading.
    type Content: Into<AttributeContent>;

    /// Reads the parts, or `Ok(None)` when the source carried no value.
    ///
    /// # Errors
    /// Returns the [`AttributeRefusal`] naming what the reading could not use.
    fn read(self, parts: SourceParts, ctx: &mut ReadContext<'_>) -> Result<Option<Self::Reading>, AttributeRefusal>;

    /// Admits what was read as the content of the type, counting in `omitted` each null it leaves
    /// out, or `Ok(None)` when what was read carries no value.
    ///
    /// # Errors
    /// Returns the [`AttributeRefusal`] naming why the type cannot hold what was read.
    fn admit(self, reading: Self::Reading, omitted: &mut OmittedNulls) -> Result<Option<Self::Content>, AttributeRefusal>;

    /// The raw JSON a reading contributes to an object nested `mappings` assemble.
    fn into_json(reading: Self::Reading) -> JsonValue;
}

/// What converting a declaration's source values produces where the declaration is resolved.
pub(crate) trait ConversionOutput: From<AttributeContent> {
    /// Converts the parts through `conversion`, or `Ok(None)` when the source carried no value.
    ///
    /// # Errors
    /// Returns the [`AttributeRefusal`] naming what the declaration could not take.
    fn convert<C: AttributeConversion>(conversion: C, parts: SourceParts, ctx: &mut ReadContext<'_>) -> Result<Option<Self>, AttributeRefusal>;
}

/// An attribute's own content: what was read, admitted by the attribute's type.
impl ConversionOutput for AttributeContent {
    fn convert<C: AttributeConversion>(conversion: C, parts: SourceParts, ctx: &mut ReadContext<'_>) -> Result<Option<AttributeContent>, AttributeRefusal> {
        Ok(conversion
            .read(parts, ctx)?
            .map_or(Ok(None), |reading| conversion.admit(reading, ctx.omitted))?
            .map(Into::into))
    }
}

/// A member of an object nested `mappings` assemble: what was read, not admitted by the member's
/// own type, since the type of the attribute holding the object admits it once.
impl ConversionOutput for JsonValue {
    fn convert<C: AttributeConversion>(conversion: C, parts: SourceParts, ctx: &mut ReadContext<'_>) -> Result<Option<JsonValue>, AttributeRefusal> {
        Ok(conversion.read(parts, ctx)?.map(C::into_json))
    }
}
