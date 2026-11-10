use netcdf_reader::{NcDimension, NcType, NcVariable};

/// What kind of values a `NetCDF` variable holds, as far as a location record is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// Integers or floats, decoded as `f64`.
    Numeric,
    /// Characters or strings, decoded as text.
    Text,
    /// A `NetCDF-4` user-defined type (enum, compound, opaque, array, variable-length), which has no
    /// record representation.
    UserDefined,
}

impl ValueKind {
    /// The kind of values a variable of `dtype` holds.
    #[must_use]
    pub const fn of(dtype: &NcType) -> ValueKind {
        match dtype {
            NcType::Byte
            | NcType::Short
            | NcType::Int
            | NcType::Float
            | NcType::Double
            | NcType::UByte
            | NcType::UShort
            | NcType::UInt
            | NcType::Int64
            | NcType::UInt64 => ValueKind::Numeric,
            NcType::Char | NcType::String => ValueKind::Text,
            NcType::Enum { .. } | NcType::Compound { .. } | NcType::Opaque { .. } | NcType::Array { .. } | NcType::VLen { .. } => ValueKind::UserDefined,
        }
    }
}

/// The dimensions a variable's values range over, one value per index combination.
///
/// A classic character array stores each string along its last dimension (NUG, "Strings"), so that
/// dimension is the string length rather than a dimension of the values.
#[must_use]
pub fn value_dimensions(variable: &NcVariable) -> &[NcDimension] {
    match variable.dimensions.split_last() {
        Some((_string_length, leading)) if variable.dtype == NcType::Char => leading,
        Some(_) | None => &variable.dimensions,
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::value_kind::{ValueKind, value_dimensions};
    use netcdf_reader::{NcDimension, NcType, NcVariable};
    use pretty_assertions::assert_eq;

    fn dimension(name: &str, size: u64) -> NcDimension {
        NcDimension {
            name: name.to_string(),
            size,
            is_unlimited: false,
        }
    }

    fn variable(dtype: NcType, dimensions: Vec<NcDimension>) -> NcVariable {
        NcVariable {
            name: "v".to_string(),
            dimensions,
            dtype,
            attributes: Vec::new(),
            data_offset: 0,
            _data_size: 0,
            is_record_var: false,
            record_size: 0,
        }
    }

    #[test]
    fn numeric_text_and_user_defined_types_are_told_apart() {
        assert_eq!(ValueKind::of(&NcType::Short), ValueKind::Numeric);
        assert_eq!(ValueKind::of(&NcType::Char), ValueKind::Text);
        assert_eq!(ValueKind::of(&NcType::String), ValueKind::Text);
        assert_eq!(ValueKind::of(&NcType::VLen { base: Box::new(NcType::Int) }), ValueKind::UserDefined);
    }

    #[test]
    fn a_character_array_ranges_over_all_but_its_string_length_dimension() {
        let names = variable(NcType::Char, vec![dimension("station", 2), dimension("name_strlen", 8)]);
        assert_eq!(value_dimensions(&names), &[dimension("station", 2)]);
    }

    #[test]
    fn a_numeric_or_string_variable_ranges_over_all_its_dimensions() {
        let data = variable(NcType::Float, vec![dimension("station", 2), dimension("time", 3)]);
        assert_eq!(value_dimensions(&data).len(), 2);
        let ids = variable(NcType::String, vec![dimension("station", 2)]);
        assert_eq!(value_dimensions(&ids).len(), 1);
    }
}
