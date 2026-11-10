use crate::netcdf::cf_attribute::CfAttribute;
use netcdf_reader::{NcAttrValue, NcAttribute, NcType, NcVariable};

/// The floating-point type a numeric `NetCDF` variable's values take once read and unpacked.
///
/// CF 1.11 §8.1 unpacks packed data to the type of its `scale_factor` and `add_offset`, and only into
/// `float` from `byte`, `unsigned byte`, `short` or `unsigned short`. Packed data whose types the
/// section does not allow, such as an `int` with `float` attributes, is unpacked to `double`, as the
/// section suggests, to keep its precision; netCDF4-python and xarray do the same.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnpackedType {
    /// Single precision: a `float` variable that is not packed, or a `byte`, `unsigned byte`, `short`,
    /// `unsigned short` or `float` variable packed by `float` attributes. A `float` variable packed
    /// by `float` attributes keeps its own type, as the NUG states for attributes of the variable's type.
    Float(Option<FloatPacking>),
    /// Double precision: every other numeric variable, packed by its attributes widened to `double`.
    Double,
}

impl UnpackedType {
    /// The type `variable`'s values are unpacked to.
    #[must_use]
    pub fn of(variable: &NcVariable) -> UnpackedType {
        match Packing::of(variable.attributes()) {
            Packing::Absent => match variable.dtype() {
                NcType::Float => UnpackedType::Float(None),
                NcType::Byte
                | NcType::Short
                | NcType::Int
                | NcType::Double
                | NcType::UByte
                | NcType::UShort
                | NcType::UInt
                | NcType::Int64
                | NcType::UInt64
                | NcType::Char
                | NcType::String
                | NcType::Enum { .. }
                | NcType::Compound { .. }
                | NcType::Opaque { .. }
                | NcType::Array { .. }
                | NcType::VLen { .. } => UnpackedType::Double,
            },
            Packing::Float(packing) => match variable.dtype() {
                NcType::Byte | NcType::UByte | NcType::Short | NcType::UShort | NcType::Float => UnpackedType::Float(Some(packing)),
                NcType::Int
                | NcType::Double
                | NcType::UInt
                | NcType::Int64
                | NcType::UInt64
                | NcType::Char
                | NcType::String
                | NcType::Enum { .. }
                | NcType::Compound { .. }
                | NcType::Opaque { .. }
                | NcType::Array { .. }
                | NcType::VLen { .. } => UnpackedType::Double,
            },
            Packing::Double => UnpackedType::Double,
        }
    }
}

/// The `float` packing attributes of a variable, each applied only when declared, as CF 1.11 §8.1
/// reads them: the data are scaled before the offset is added.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatPacking {
    /// A `scale_factor` alone.
    Scale { scale_factor: f32 },
    /// An `add_offset` alone.
    Offset { add_offset: f32 },
    /// Both attributes.
    ScaleAndOffset { scale_factor: f32, add_offset: f32 },
}

impl FloatPacking {
    /// Unpacks one stored value in single precision.
    ///
    /// Each operation rounds to `float`, as netCDF4-python and xarray compute it, and none is fused,
    /// so the result is the value those readers show.
    #[must_use]
    pub const fn unpack(self, stored: f32) -> f32 {
        match self {
            FloatPacking::Scale { scale_factor } => stored * scale_factor,
            FloatPacking::Offset { add_offset } => stored + add_offset,
            FloatPacking::ScaleAndOffset { scale_factor, add_offset } => stored * scale_factor + add_offset,
        }
    }
}

/// What a variable's `scale_factor` and `add_offset` together unpack to.
enum Packing {
    /// Neither attribute holds a number, so the variable is not packed.
    Absent,
    /// Every attribute holding a number is a `float`.
    Float(FloatPacking),
    /// An attribute is a `double`, or of a type CF 1.11 §8.1 does not allow.
    Double,
}

impl Packing {
    fn of(attributes: &[NcAttribute]) -> Packing {
        let scale_factor = PackingAttribute::of(CfAttribute::ScaleFactor.value(attributes));
        let add_offset = PackingAttribute::of(CfAttribute::AddOffset.value(attributes));
        match (scale_factor, add_offset) {
            (PackingAttribute::Absent, PackingAttribute::Absent) => Packing::Absent,
            (PackingAttribute::Float(scale_factor), PackingAttribute::Absent) => Packing::Float(FloatPacking::Scale { scale_factor }),
            (PackingAttribute::Absent, PackingAttribute::Float(add_offset)) => Packing::Float(FloatPacking::Offset { add_offset }),
            (PackingAttribute::Float(scale_factor), PackingAttribute::Float(add_offset)) => {
                Packing::Float(FloatPacking::ScaleAndOffset { scale_factor, add_offset })
            }
            (PackingAttribute::Other, PackingAttribute::Absent | PackingAttribute::Float(_) | PackingAttribute::Other)
            | (PackingAttribute::Absent | PackingAttribute::Float(_), PackingAttribute::Other) => Packing::Double,
        }
    }
}

/// What one packing attribute holds.
///
/// An attribute is read as the netcdf-reader unpacks it, by its first number, so one that holds no
/// number, such as text, packs nothing.
enum PackingAttribute {
    /// The attribute is undeclared or holds no number.
    Absent,
    /// A `float`.
    Float(f32),
    /// A number of any other type.
    Other,
}

impl PackingAttribute {
    fn of(value: Option<&NcAttrValue>) -> PackingAttribute {
        match value {
            None => PackingAttribute::Absent,
            Some(NcAttrValue::Floats(values)) => values.first().map_or(PackingAttribute::Absent, |value| PackingAttribute::Float(*value)),
            Some(
                value @ (NcAttrValue::Bytes(_)
                | NcAttrValue::Chars(_)
                | NcAttrValue::Shorts(_)
                | NcAttrValue::Ints(_)
                | NcAttrValue::Doubles(_)
                | NcAttrValue::UBytes(_)
                | NcAttrValue::UShorts(_)
                | NcAttrValue::UInts(_)
                | NcAttrValue::Int64s(_)
                | NcAttrValue::UInt64s(_)
                | NcAttrValue::Strings(_)
                | NcAttrValue::UserDefined { .. }),
            ) => match value.as_f64() {
                Some(_) => PackingAttribute::Other,
                None => PackingAttribute::Absent,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::unpacked_type::{FloatPacking, UnpackedType};
    use netcdf_reader::{NcAttrValue, NcAttribute, NcType, NcVariable};
    use pretty_assertions::assert_eq;

    fn unpacked(dtype: NcType, attributes: Vec<(&str, NcAttrValue)>) -> UnpackedType {
        UnpackedType::of(&NcVariable {
            name: "v".to_string(),
            dimensions: Vec::new(),
            dtype,
            attributes: attributes
                .into_iter()
                .map(|(name, value)| NcAttribute { name: name.to_string(), value })
                .collect(),
            data_offset: 0,
            _data_size: 0,
            is_record_var: false,
            record_size: 0,
        })
    }

    fn float_scale() -> Vec<(&'static str, NcAttrValue)> {
        vec![("scale_factor", NcAttrValue::Floats(vec![0.01]))]
    }

    #[test]
    fn an_unpacked_float_is_single_precision_and_every_other_type_double() {
        assert_eq!(unpacked(NcType::Float, Vec::new()), UnpackedType::Float(None));
        assert_eq!(unpacked(NcType::Double, Vec::new()), UnpackedType::Double);
        assert_eq!(unpacked(NcType::Short, Vec::new()), UnpackedType::Double);
        assert_eq!(unpacked(NcType::Int, Vec::new()), UnpackedType::Double);
    }

    #[test]
    fn a_short_packed_by_float_attributes_is_unpacked_to_float() {
        assert_eq!(
            unpacked(
                NcType::Short,
                vec![
                    ("scale_factor", NcAttrValue::Floats(vec![0.01])),
                    ("add_offset", NcAttrValue::Floats(vec![20.0]))
                ]
            ),
            UnpackedType::Float(Some(FloatPacking::ScaleAndOffset {
                scale_factor: 0.01,
                add_offset: 20.0
            }))
        );
    }

    #[test]
    fn every_type_cf_lets_float_data_be_packed_into_unpacks_to_float() {
        for dtype in [NcType::Byte, NcType::UByte, NcType::Short, NcType::UShort] {
            assert_eq!(
                unpacked(dtype, vec![("scale_factor", NcAttrValue::Floats(vec![0.5]))]),
                UnpackedType::Float(Some(FloatPacking::Scale { scale_factor: 0.5 }))
            );
        }
    }

    #[test]
    fn an_offset_alone_packs_by_itself() {
        assert_eq!(
            unpacked(NcType::UShort, vec![("add_offset", NcAttrValue::Floats(vec![-40.0]))]),
            UnpackedType::Float(Some(FloatPacking::Offset { add_offset: -40.0 }))
        );
    }

    #[test]
    fn a_float_packed_by_float_attributes_keeps_its_own_type() {
        assert_eq!(
            unpacked(NcType::Float, vec![("scale_factor", NcAttrValue::Floats(vec![2.0]))]),
            UnpackedType::Float(Some(FloatPacking::Scale { scale_factor: 2.0 }))
        );
    }

    #[test]
    fn double_attributes_unpack_to_double() {
        assert_eq!(
            unpacked(NcType::Short, vec![("scale_factor", NcAttrValue::Doubles(vec![0.01]))]),
            UnpackedType::Double
        );
        assert_eq!(
            unpacked(NcType::Float, vec![("add_offset", NcAttrValue::Doubles(vec![1.0]))]),
            UnpackedType::Double
        );
    }

    #[test]
    fn float_attributes_on_a_type_cf_does_not_pack_float_into_unpack_to_double() {
        assert_eq!(unpacked(NcType::Int, float_scale()), UnpackedType::Double);
        assert_eq!(unpacked(NcType::UInt, float_scale()), UnpackedType::Double);
        assert_eq!(unpacked(NcType::Int64, float_scale()), UnpackedType::Double);
        assert_eq!(unpacked(NcType::Double, float_scale()), UnpackedType::Double);
    }

    #[test]
    fn attributes_of_different_types_unpack_to_double() {
        assert_eq!(
            unpacked(
                NcType::Short,
                vec![
                    ("scale_factor", NcAttrValue::Floats(vec![0.01])),
                    ("add_offset", NcAttrValue::Doubles(vec![20.0]))
                ]
            ),
            UnpackedType::Double
        );
        assert_eq!(
            unpacked(
                NcType::Short,
                vec![("scale_factor", NcAttrValue::Shorts(vec![2])), ("add_offset", NcAttrValue::Floats(vec![20.0]))]
            ),
            UnpackedType::Double
        );
    }

    #[test]
    fn an_attribute_holding_no_number_packs_nothing() {
        assert_eq!(
            unpacked(NcType::Float, vec![("scale_factor", NcAttrValue::Chars("0.5".to_string()))]),
            UnpackedType::Float(None)
        );
        assert_eq!(
            unpacked(NcType::Float, vec![("add_offset", NcAttrValue::Floats(Vec::new()))]),
            UnpackedType::Float(None)
        );
        assert_eq!(
            unpacked(NcType::Float, vec![("add_offset", NcAttrValue::Doubles(Vec::new()))]),
            UnpackedType::Float(None)
        );
    }

    #[test]
    fn a_packed_value_is_scaled_then_offset_in_single_precision() {
        assert_eq!(
            FloatPacking::ScaleAndOffset {
                scale_factor: 0.01,
                add_offset: 20.0
            }
            .unpack(-167.0)
            .to_bits(),
            (-167.0_f32 * 0.01 + 20.0).to_bits()
        );
        assert_eq!(FloatPacking::Scale { scale_factor: 0.01 }.unpack(-167.0).to_bits(), (-1.67_f32).to_bits());
        assert_eq!(FloatPacking::Offset { add_offset: 0.5 }.unpack(2.0).to_bits(), 2.5_f32.to_bits());
    }

    #[test]
    fn a_scale_alone_adds_no_offset_so_a_negative_zero_keeps_its_sign() {
        assert!(FloatPacking::Scale { scale_factor: -0.5 }.unpack(0.0).is_sign_negative());
    }
}
