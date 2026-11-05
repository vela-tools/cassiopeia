use crate::attribute::{
    conversion::{self, readers::SourceParts},
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

/// Applies a conversion with a fresh null count, which the caller does not inspect.
pub(crate) fn apply(parts: SourceParts, conversion: Conversion, policy: Option<&GeometryPolicy>) -> Result<Option<AttributeContent>, AttributeRefusal> {
    conversion::apply(parts, conversion, policy, &mut OmittedNulls::default())
}

/// Applies a conversion expected to succeed with an NGSI-LD Value or with no value.
pub(crate) fn converted(parts: SourceParts, conversion: Conversion, policy: Option<&GeometryPolicy>) -> Option<Value> {
    match apply(parts, conversion, policy).unwrap() {
        Some(AttributeContent::Value(value)) => Some(value),
        Some(other @ (AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_))) => panic!("{other:?}, not a value"),
        None => None,
    }
}

/// Applies a conversion expected to succeed with raw JSON or with no value.
pub(crate) fn payload(parts: SourceParts, conversion: Conversion) -> Option<JsonValue> {
    match apply(parts, conversion, None).unwrap() {
        Some(AttributeContent::Json(payload)) => Some(JsonValue::from(payload)),
        Some(other @ (AttributeContent::Value(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_))) => panic!("{other:?}, not raw JSON"),
        None => None,
    }
}

/// Applies a conversion and returns what it produced with the nulls it left out of the value.
pub(crate) fn applied_counting(parts: SourceParts, conversion: Conversion) -> (Result<Option<AttributeContent>, AttributeRefusal>, OmittedNulls) {
    let mut omitted = OmittedNulls::default();
    let content = conversion::apply(parts, conversion, None, &mut omitted);
    (content, omitted)
}

/// Applies a conversion expected to succeed with an NGSI-LD Value or with no value, and returns it
/// with the nulls it left out of the value.
pub(crate) fn omitted(parts: SourceParts, conversion: Conversion) -> (Option<Value>, OmittedNulls) {
    let (content, omitted) = applied_counting(parts, conversion);
    let value = match content.unwrap() {
        Some(AttributeContent::Value(value)) => Some(value),
        Some(other @ (AttributeContent::Json(_) | AttributeContent::Vocab(_) | AttributeContent::Language(_))) => panic!("{other:?}, not a value"),
        None => None,
    };
    (value, omitted)
}
