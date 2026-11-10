use derive_more::{Display, From};

/// The name of a `NetCDF` variable, as the file declares it.
///
/// A newtype rather than a bare string so a variable named in a report cannot be confused with the
/// record key its values are emitted under, which for a time coordinate is `time` whatever the
/// variable is called.
#[derive(Debug, PartialEq, Eq, Display, From)]
#[from(forward)]
pub struct VariableName(String);

#[cfg(test)]
mod tests {
    use crate::netcdf::variable_name::VariableName;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_variable_name_is_written_as_the_file_declares_it() {
        assert_eq!(VariableName::from("valid_time"), VariableName::from("valid_time".to_owned()));
        assert_eq!(VariableName::from("valid_time").to_string(), "valid_time");
    }
}
