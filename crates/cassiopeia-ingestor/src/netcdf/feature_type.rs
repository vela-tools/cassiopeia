use crate::netcdf::{cf_attribute::CfAttribute, error::NetCdfIngestError};
use netcdf_reader::NcAttribute;

/// The shape of a `NetCDF` file's data, from its global `featureType` attribute (CF 1.11 §9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureType {
    /// No `featureType`, or the `GRID` that netCDF-Java (THREDDS, its `NetCDF` Subset Service) writes
    /// on the grids it produces: data variables on latitude and longitude grid axes.
    Grid,
    /// A discrete sampling geometry of time series at fixed stations (CF 1.11 §9.1, Appendix H.2).
    TimeSeries,
}

impl FeatureType {
    /// The feature type the global attributes declare. CF compares feature type names
    /// case-insensitively (CF 1.11 §9.4). CF names only discrete sampling geometries, so a grid
    /// declares none, but netCDF-Java labels every grid it writes with its own CDM feature type
    /// `GRID`, which is read as a grid too.
    ///
    /// # Errors
    ///
    /// Returns [`NetCdfIngestError::UnsupportedFeatureType`] for any feature type other than `grid`
    /// and `timeSeries`.
    pub fn of(global_attributes: &[NcAttribute]) -> Result<FeatureType, NetCdfIngestError> {
        match CfAttribute::FeatureType.text(global_attributes) {
            None => Ok(FeatureType::Grid),
            Some(name) if name.eq_ignore_ascii_case("grid") => Ok(FeatureType::Grid),
            Some(name) if name.eq_ignore_ascii_case("timeSeries") => Ok(FeatureType::TimeSeries),
            Some(name) => Err(NetCdfIngestError::UnsupportedFeatureType { feature_type: name.to_owned() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::{error::NetCdfIngestError, feature_type::FeatureType};
    use netcdf_reader::{NcAttrValue, NcAttribute};
    use pretty_assertions::assert_eq;

    fn declared(name: &str) -> Result<FeatureType, NetCdfIngestError> {
        FeatureType::of(&[NcAttribute {
            name: "featureType".to_string(),
            value: NcAttrValue::Chars(name.to_string()),
        }])
    }

    #[test]
    fn a_file_without_a_feature_type_is_a_grid() {
        assert_eq!(FeatureType::of(&[]).unwrap(), FeatureType::Grid);
    }

    #[test]
    fn time_series_is_recognized_in_any_case() {
        assert_eq!(declared("timeSeries").unwrap(), FeatureType::TimeSeries);
        assert_eq!(declared("timeseries").unwrap(), FeatureType::TimeSeries);
    }

    #[test]
    fn the_grid_feature_type_netcdf_java_writes_is_a_grid() {
        assert_eq!(declared("GRID").unwrap(), FeatureType::Grid);
        assert_eq!(declared("grid").unwrap(), FeatureType::Grid);
    }

    #[test]
    fn another_feature_type_is_unsupported() {
        assert!(matches!(declared("trajectory"), Err(NetCdfIngestError::UnsupportedFeatureType { .. })));
    }
}
