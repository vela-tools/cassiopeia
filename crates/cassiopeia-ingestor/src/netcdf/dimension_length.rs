use crate::netcdf::error::NetCdfIngestError;
use netcdf_reader::NcDimension;

/// A dimension's length as an index bound on this platform.
///
/// # Errors
///
/// Returns [`NetCdfIngestError::DimensionTooLong`] when the length does not fit a `usize`.
pub fn dimension_length(dimension: &NcDimension) -> Result<usize, NetCdfIngestError> {
    usize::try_from(dimension.size).map_err(|_| too_long(dimension))
}

/// The error for a dimension too long to index on this platform.
#[must_use]
pub fn too_long(dimension: &NcDimension) -> NetCdfIngestError {
    NetCdfIngestError::DimensionTooLong {
        // The error outlives the file's metadata, so it keeps its own copy of the name.
        dimension: dimension.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::dimension_length::dimension_length;
    use netcdf_reader::NcDimension;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_dimension_length_is_its_size_as_an_index_bound() {
        let dimension = NcDimension {
            name: "time".to_string(),
            size: 24,
            is_unlimited: true,
        };
        assert_eq!(dimension_length(&dimension).unwrap(), 24);
    }
}
