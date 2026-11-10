use crate::netcdf::{
    cf_attribute::CfAttribute,
    coordinate_facets::CoordinateKey,
    coordinate_role::CoordinateRole,
    error::NetCdfIngestError,
    value_kind::ValueKind,
};
use netcdf_reader::{NcGroup, NcVariable};
use std::collections::HashSet;

/// The `cf_role` value that marks a time series' station identifier (CF 1.11 §9.5).
const TIMESERIES_ID: &str = "timeseries_id";

/// The coordinates of a `timeSeries` file: the station position, the observation time, and every
/// other coordinate a record carries.
///
/// They are found among the variables a data variable names in its `coordinates` attribute, the
/// coordinate variables, and the variable whose `cf_role` is `timeseries_id` (CF 1.11 §9.5). The
/// first latitude, longitude and time found locate each observation, and the first vertical
/// coordinate that maps onto a canonical level becomes the level; every other one is emitted under
/// its own name, so a station's name, identifier and any further listed coordinate reach the record.
/// A coordinate holding a `NetCDF-4` user-defined type has no record representation and is skipped
/// with a warning.
#[derive(Debug, Clone)]
pub struct TimeSeriesCoordinates<'f> {
    /// The station latitude.
    pub latitude: &'f NcVariable,
    /// The station longitude.
    pub longitude: &'f NcVariable,
    /// The observation time.
    pub time: &'f NcVariable,
    /// Every other coordinate, with where its value lands in the record.
    pub facets: Vec<(&'f NcVariable, CoordinateKey)>,
}

impl<'f> TimeSeriesCoordinates<'f> {
    /// Finds the coordinates of the root group's time series.
    ///
    /// # Errors
    ///
    /// Returns [`NetCdfIngestError::MissingTimeSeriesCoordinate`] when no latitude, longitude or time
    /// coordinate is found.
    pub fn find(root: &'f NcGroup) -> Result<TimeSeriesCoordinates<'f>, NetCdfIngestError> {
        let listed: HashSet<&str> = root
            .variables
            .iter()
            .flat_map(|variable| CfAttribute::Coordinates.names(variable.attributes()))
            .collect();
        let candidates = root
            .variables
            .iter()
            .filter(|variable| listed.contains(variable.name()) || variable.is_coordinate_variable() || is_timeseries_id(variable))
            .filter(|variable| {
                let readable = ValueKind::of(variable.dtype()) != ValueKind::UserDefined;
                if !readable {
                    tracing::warn!(variable = variable.name(), "NetCDF timeSeries coordinate skipped: it holds a user-defined type");
                }
                readable
            });

        let mut latitude = None;
        let mut longitude = None;
        let mut time = None;
        let mut facets: Vec<(&NcVariable, CoordinateKey)> = Vec::new();
        for variable in candidates {
            let role = CoordinateRole::of(variable.attributes());
            let has_level = facets.iter().any(|(_, key)| matches!(key, CoordinateKey::Level(_)));
            match role {
                CoordinateRole::Latitude if latitude.is_none() => latitude = Some(variable),
                CoordinateRole::Longitude if longitude.is_none() => longitude = Some(variable),
                CoordinateRole::Time if time.is_none() => time = Some(variable),
                CoordinateRole::Vertical if !has_level => facets.push((variable, CoordinateKey::of(root, variable, role))),
                CoordinateRole::Latitude
                | CoordinateRole::Longitude
                | CoordinateRole::Time
                | CoordinateRole::Vertical
                | CoordinateRole::ProjectedX
                | CoordinateRole::ProjectedY
                | CoordinateRole::Other => facets.push((variable, CoordinateKey::of(root, variable, CoordinateRole::Other))),
            }
        }

        let missing = |role| NetCdfIngestError::MissingTimeSeriesCoordinate { role };
        Ok(TimeSeriesCoordinates {
            latitude: latitude.ok_or_else(|| missing(CoordinateRole::Latitude))?,
            longitude: longitude.ok_or_else(|| missing(CoordinateRole::Longitude))?,
            time: time.ok_or_else(|| missing(CoordinateRole::Time))?,
            facets,
        })
    }

    /// Whether `variable` is one of these coordinates.
    #[must_use]
    pub fn contains(&self, variable: &NcVariable) -> bool {
        [self.latitude, self.longitude, self.time]
            .into_iter()
            .chain(self.facets.iter().map(|(coordinate, _)| *coordinate))
            .any(|coordinate| coordinate.name() == variable.name())
    }
}

/// Whether `variable` identifies the time series' stations.
fn is_timeseries_id(variable: &NcVariable) -> bool {
    CfAttribute::CfRole.text(variable.attributes()) == Some(TIMESERIES_ID)
}
