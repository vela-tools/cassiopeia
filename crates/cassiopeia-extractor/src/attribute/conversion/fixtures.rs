use crate::attribute::{
    conversion::{self, attribute_conversion::ConversionOutput, read_context::ReadContext, source_parts::SourceParts},
    refusal::AttributeRefusal,
};
use cassiopeia_geometry::policy::GeometryPolicy;
use cassiopeia_ir::attribute_content::AttributeContent;
use cassiopeia_mapping::{
    conversion::{
        Conversion,
        declaration_shape::{DeclarationShape, ValueOrigin},
    },
    transformation::Transformation,
};
use cassiopeia_ngsi_ld::{
    entity::attribute::NgsiLdAttributeKind,
    value::{omitted_nulls::OmittedNulls, types::Value},
};
use serde_json::{Value as JsonValue, json};
use smallvec::smallvec;

/// The shape of a declaration read from a `source`.
const SOURCED: DeclarationShape = DeclarationShape::Value(ValueOrigin::Sourced);

/// The conversion a declaration of `kind` read from a source and naming no transformation loads
/// with.
pub(crate) fn default_of(kind: NgsiLdAttributeKind) -> Conversion {
    Conversion::resolve(kind, None, SOURCED).unwrap()
}

/// The conversion a declaration of `kind` read from a source under `transformation` loads with.
pub(crate) fn declared(kind: NgsiLdAttributeKind, transformation: Transformation) -> Conversion {
    Conversion::resolve(kind, Some(transformation), SOURCED).unwrap()
}

/// The conversion a `Property` read from a source under `transformation` loads with.
pub(crate) fn property(transformation: Transformation) -> Conversion {
    declared(NgsiLdAttributeKind::Property, transformation)
}

/// The conversion a declaration of `kind` assembled from nested `mappings` under `transformation`,
/// or none, loads with.
pub(crate) fn assembled(kind: NgsiLdAttributeKind, transformation: Option<Transformation>) -> Conversion {
    Conversion::resolve(kind, transformation, DeclarationShape::Value(ValueOrigin::Assembled)).unwrap()
}

/// The conversion a `Property` assembled from nested `mappings` under `transformation` loads with.
pub(crate) fn assembled_property(transformation: Transformation) -> Conversion {
    assembled(NgsiLdAttributeKind::Property, Some(transformation))
}

/// The one part the object `{"street": "Ring"}` is, as nested `mappings` would assemble it.
pub(crate) fn street() -> SourceParts {
    smallvec![json!({"street": "Ring"})]
}

/// Converts the parts into `O` under `policy`, or the lossless default as the resolver settles it,
/// and returns what it produced with the nulls it counted.
fn converting<O: ConversionOutput>(
    parts: SourceParts,
    conversion: Conversion,
    policy: Option<&GeometryPolicy>,
) -> (Result<Option<O>, AttributeRefusal>, OmittedNulls) {
    let mut omitted = OmittedNulls::default();
    let output = conversion::convert(
        parts,
        conversion,
        &mut ReadContext {
            policy: policy.copied().unwrap_or_default(),
            omitted: &mut omitted,
        },
    );
    (output, omitted)
}

/// Applies a conversion with a fresh null count, which the caller does not inspect.
pub(crate) fn apply(parts: SourceParts, conversion: Conversion, policy: Option<&GeometryPolicy>) -> Result<Option<AttributeContent>, AttributeRefusal> {
    converting(parts, conversion, policy).0
}

/// Reads the parts as a member of an object nested `mappings` assemble, and returns the raw JSON it
/// contributes with the nulls it counted.
pub(crate) fn member(parts: SourceParts, conversion: Conversion) -> (Result<Option<JsonValue>, AttributeRefusal>, OmittedNulls) {
    converting(parts, conversion, None)
}

/// Applies a conversion expected to succeed with an NGSI-LD Value or with no value.
pub(crate) fn converted(parts: SourceParts, conversion: Conversion, policy: Option<&GeometryPolicy>) -> Option<Value> {
    apply(parts, conversion, policy).unwrap().map(|content| content.try_unwrap_value().unwrap())
}

/// Applies a conversion expected to succeed with raw JSON or with no value.
pub(crate) fn payload(parts: SourceParts, conversion: Conversion) -> Option<JsonValue> {
    apply(parts, conversion, None)
        .unwrap()
        .map(|content| JsonValue::from(content.try_unwrap_json().unwrap()))
}

/// Applies a conversion and returns what it produced with the nulls it left out of the value.
pub(crate) fn applied_counting(parts: SourceParts, conversion: Conversion) -> (Result<Option<AttributeContent>, AttributeRefusal>, OmittedNulls) {
    converting(parts, conversion, None)
}

/// Applies a conversion expected to succeed with an NGSI-LD Value or with no value, and returns it
/// with the nulls it left out of the value.
pub(crate) fn omitted(parts: SourceParts, conversion: Conversion) -> (Option<Value>, OmittedNulls) {
    let (content, omitted) = applied_counting(parts, conversion);
    (content.unwrap().map(|content| content.try_unwrap_value().unwrap()), omitted)
}
