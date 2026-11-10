use crate::netcdf::error::NetCdfIngestError;
use std::collections::HashSet;

/// One key a `NetCDF` location record carries, with the variable that produces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordKey<'k> {
    /// The key in the record.
    pub key: &'k str,
    /// The variable whose value is emitted under the key.
    pub variable: &'k str,
}

impl<'k> RecordKey<'k> {
    /// The key `variable` emits its value under.
    #[must_use]
    pub const fn new(key: &'k str, variable: &'k str) -> RecordKey<'k> {
        RecordKey { key, variable }
    }
}

/// Fails when two of a location record's keys coincide, since one would silently overwrite the other.
///
/// # Errors
///
/// Returns [`NetCdfIngestError::ColumnCollision`] naming the variable whose key is already taken.
pub fn ensure_distinct<'k>(keys: impl IntoIterator<Item = RecordKey<'k>>) -> Result<(), NetCdfIngestError> {
    let mut seen = HashSet::new();
    keys.into_iter().try_for_each(|key| {
        if seen.insert(key.key) {
            Ok(())
        } else {
            Err(NetCdfIngestError::ColumnCollision {
                variable: key.variable.to_owned(),
            })
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::netcdf::{
        error::NetCdfIngestError,
        record_keys::{RecordKey, ensure_distinct},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn distinct_keys_pass() {
        assert!(ensure_distinct([RecordKey::new("latitude", "lat"), RecordKey::new("t", "t")]).is_ok());
    }

    #[test]
    fn a_repeated_key_names_the_variable_that_repeats_it() {
        let error = ensure_distinct([RecordKey::new("time", "t"), RecordKey::new("time", "time")]).unwrap_err();
        let NetCdfIngestError::ColumnCollision { variable } = error else {
            panic!("expected a column collision, got {error:?}");
        };
        assert_eq!(variable, "time");
    }
}
