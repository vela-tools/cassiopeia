use crate::netcdf::cf_attribute::CfAttribute;
use netcdf_core::{NC_FILL_DOUBLE, NC_FILL_FLOAT, NC_FILL_INT, NC_FILL_INT64, NC_FILL_SHORT, NC_FILL_UINT, NC_FILL_UINT64, NC_FILL_USHORT};
use netcdf_reader::{NcType, NcVariable};
use num_traits::ToPrimitive;

/// The netCDF default fill value standing for an unwritten element of `variable`, as the stored
/// value promoted to `f64`, when the variable declares no `_FillValue` of its own.
///
/// The netCDF library pre-fills every element a writer never sets with its type's default fill
/// value, and generic readers (`ncdump`, netCDF4-python) treat that value as missing when no
/// `_FillValue` attribute overrides it. The byte types are left out, as netCDF4-python leaves them
/// out: their whole range is commonly real data, such as flags and counts. A variable that
/// declares `_FillValue` is masked by that attribute alone.
#[must_use]
pub fn default_fill(variable: &NcVariable) -> Option<f64> {
    if CfAttribute::FillValue.is_declared(variable.attributes()) {
        return None;
    }
    match variable.dtype() {
        NcType::Short => Some(f64::from(NC_FILL_SHORT)),
        NcType::Int => Some(f64::from(NC_FILL_INT)),
        NcType::Float => Some(f64::from(NC_FILL_FLOAT)),
        NcType::Double => Some(NC_FILL_DOUBLE),
        NcType::UShort => Some(f64::from(NC_FILL_USHORT)),
        NcType::UInt => Some(f64::from(NC_FILL_UINT)),
        NcType::Int64 => NC_FILL_INT64.to_f64(),
        NcType::UInt64 => NC_FILL_UINT64.to_f64(),
        NcType::Byte
        | NcType::UByte
        | NcType::Char
        | NcType::String
        | NcType::Enum { .. }
        | NcType::Compound { .. }
        | NcType::Opaque { .. }
        | NcType::Array { .. }
        | NcType::VLen { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::default_fill::default_fill;
    use netcdf_reader::{NcAttrValue, NcAttribute, NcType, NcVariable};
    use pretty_assertions::assert_eq;

    fn variable(dtype: NcType, attributes: Vec<NcAttribute>) -> NcVariable {
        NcVariable {
            name: "v".to_string(),
            dimensions: Vec::new(),
            dtype,
            attributes,
            data_offset: 0,
            _data_size: 0,
            is_record_var: false,
            record_size: 0,
        }
    }

    #[test]
    fn an_integer_without_a_fill_value_attribute_uses_the_netcdf_default() {
        assert_eq!(default_fill(&variable(NcType::Int, Vec::new())), Some(-2_147_483_647.0));
        assert_eq!(default_fill(&variable(NcType::Short, Vec::new())), Some(-32_767.0));
    }

    #[test]
    fn a_float_default_fill_is_the_single_precision_value_widened() {
        assert_eq!(
            default_fill(&variable(NcType::Float, Vec::new())).map(f64::to_bits),
            Some(f64::from(9.969_21e36_f32).to_bits())
        );
    }

    #[test]
    fn a_declared_fill_value_replaces_the_default() {
        let declared = vec![NcAttribute {
            name: "_FillValue".to_string(),
            value: NcAttrValue::Ints(vec![-1]),
        }];
        assert_eq!(default_fill(&variable(NcType::Int, declared)), None);
    }

    #[test]
    fn the_byte_types_have_no_default_fill() {
        assert_eq!(default_fill(&variable(NcType::Byte, Vec::new())), None);
        assert_eq!(default_fill(&variable(NcType::UByte, Vec::new())), None);
    }
}
