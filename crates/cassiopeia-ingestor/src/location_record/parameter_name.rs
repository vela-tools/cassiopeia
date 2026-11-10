use derive_more::From;

/// The key under which a value is stored in a location record.
///
/// A newtype rather than a bare string so a column key cannot be confused with an unrelated string (a
/// level token, a timestamp) as it flows through the pivot. It holds a GRIB parameter's canonical or
/// synthetic code key, a `NetCDF` variable name, or the name of a facet such as a time.
#[derive(Debug, Clone, PartialEq, Eq, From)]
#[from(forward)]
pub struct ParameterName(String);

impl ParameterName {
    /// Returns the parameter name as a string slice, for use as a record column key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::location_record::parameter_name::ParameterName;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_name_built_from_a_slice_and_one_built_from_an_owned_string_are_equal() {
        assert_eq!(ParameterName::from("wind_u"), ParameterName::from("wind_u".to_string()));
        assert_eq!(ParameterName::from("wind_u").as_str(), "wind_u");
    }
}
