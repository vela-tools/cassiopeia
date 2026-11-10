use crate::{
    location_record::{
        facets::{LEVEL, LEVEL_TYPE, LocationFacets},
        level::Level,
        parameter_name::ParameterName,
    },
    netcdf::{
        coordinate_role::CoordinateRole,
        decode::Decode,
        depth_surface::DepthSurface,
        error::NetCdfIngestError,
        record_keys::RecordKey,
        time_axis::TimeAxis,
        value_kind::ValueKind,
        vertical_level::VerticalAxis,
    },
};
use netcdf_reader::{NcFile, NcGroup, NcVariable};
use serde_json::Value;

/// The record key a time coordinate is emitted under.
pub const TIME: &str = "time";

/// Where a coordinate's values land in a location record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinateKey {
    /// A numeric vertical coordinate that maps onto canonical levels, emitted as `level_type` and
    /// `level`.
    Level(VerticalAxis),
    /// Any other coordinate, emitted under one key: `time` for the time coordinate, its own variable
    /// name otherwise.
    Named(ParameterName),
}

impl CoordinateKey {
    /// The key a coordinate of `root` in `role` is emitted under, decided from the file's metadata alone
    /// so collisions are found before any value is read. A depth takes the surface it is measured below
    /// from the standard names of the variables it locates.
    #[must_use]
    pub fn of(root: &NcGroup, variable: &NcVariable, role: CoordinateRole) -> CoordinateKey {
        let own_name = || CoordinateKey::Named(ParameterName::from(variable.name()));
        match role {
            CoordinateRole::Vertical => match ValueKind::of(variable.dtype()) {
                ValueKind::Numeric => VerticalAxis::of(variable.attributes(), || DepthSurface::of(root, variable)).map_or_else(own_name, CoordinateKey::Level),
                ValueKind::Text | ValueKind::UserDefined => own_name(),
            },
            CoordinateRole::Time => CoordinateKey::Named(ParameterName::from(TIME)),
            CoordinateRole::Latitude | CoordinateRole::Longitude | CoordinateRole::ProjectedX | CoordinateRole::ProjectedY | CoordinateRole::Other => {
                own_name()
            }
        }
    }

    /// The record keys this coordinate fills, each attributed to `variable`.
    #[must_use]
    pub fn record_keys<'k>(&'k self, variable: &'k NcVariable) -> Vec<RecordKey<'k>> {
        match self {
            CoordinateKey::Level(_) => vec![RecordKey::new(LEVEL_TYPE, variable.name()), RecordKey::new(LEVEL, variable.name())],
            CoordinateKey::Named(name) => vec![RecordKey::new(name.as_str(), variable.name())],
        }
    }
}

/// One coordinate's values, decoded into what each record it locates carries, with the key they are
/// emitted under borrowed from the coordinate's [`CoordinateKey`].
#[derive(Debug, Clone)]
pub enum CoordinateFacets<'k> {
    /// Canonical levels; `None` where the coordinate value is missing.
    Levels(Vec<Option<Level>>),
    /// RFC 3339 timestamps.
    Times {
        /// The record key the timestamps are emitted under.
        key: &'k ParameterName,
        /// One timestamp per coordinate value; `None` where the value is missing.
        times: Vec<Option<String>>,
    },
    /// Numbers.
    Numbers {
        /// The record key the numbers are emitted under.
        key: &'k ParameterName,
        /// One number per coordinate value; `NaN` where the value is missing.
        numbers: Vec<f64>,
    },
    /// Text, such as a station name or identifier.
    Texts {
        /// The record key the text is emitted under.
        key: &'k ParameterName,
        /// One text per coordinate value, trimmed of trailing padding; empty where the value is missing.
        texts: Vec<String>,
    },
}

impl<'k> CoordinateFacets<'k> {
    /// Reads and decodes every value of a coordinate, flattened in row-major order over its value
    /// dimensions.
    ///
    /// A level key reads numbers decoded as `decode` says and makes them levels; [`CoordinateKey::of`]
    /// gives a level key to numeric coordinates only. Under a named key, text is read as strings with
    /// trailing padding trimmed, and numbers decoded as `decode` says become RFC 3339 timestamps when the
    /// variable carries CF time units (whatever its role), and plain numbers otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`NetCdfIngestError`] when the variable cannot be read, holds a user-defined type, or
    /// carries time units that cannot be decoded.
    pub fn read(file: &NcFile, variable: &NcVariable, key: &'k CoordinateKey, decode: Decode) -> Result<CoordinateFacets<'k>, NetCdfIngestError> {
        let key = match key {
            CoordinateKey::Level(axis) => {
                let levels = decode
                    .read(file, variable)?
                    .into_iter()
                    .map(|value| (!value.is_nan()).then(|| axis.level(value)));
                return Ok(CoordinateFacets::Levels(levels.collect()));
            }
            CoordinateKey::Named(key) => key,
        };
        match ValueKind::of(variable.dtype()) {
            ValueKind::Text | ValueKind::UserDefined => {
                let texts = file
                    .read_variable_as_strings(variable.name())?
                    .into_iter()
                    .map(|mut text| {
                        text.truncate(text.trim_end().len());
                        text
                    })
                    .collect();
                Ok(CoordinateFacets::Texts { key, texts })
            }
            ValueKind::Numeric => {
                let numbers = decode.read(file, variable)?;
                match TimeAxis::from_variable(variable)? {
                    Some(axis) => {
                        let times = numbers
                            .into_iter()
                            .map(|value| if value.is_nan() { Ok(None) } else { axis.rfc3339(value).map(Some) })
                            .collect::<Result<_, _>>()?;
                        Ok(CoordinateFacets::Times { key, times })
                    }
                    None => Ok(CoordinateFacets::Numbers { key, numbers }),
                }
            }
        }
    }

    /// Whether the coordinate has a value at `index`.
    #[must_use]
    pub fn has_value(&self, index: usize) -> bool {
        match self {
            CoordinateFacets::Levels(levels) => levels.get(index).is_some_and(Option::is_some),
            CoordinateFacets::Times { times, .. } => times.get(index).is_some_and(Option::is_some),
            CoordinateFacets::Numbers { numbers, .. } => numbers.get(index).is_some_and(|number| !number.is_nan()),
            CoordinateFacets::Texts { texts, .. } => texts.get(index).is_some_and(|text| !text.is_empty()),
        }
    }

    /// Adds the coordinate's value at `index` to `facets`; a missing value adds nothing.
    #[must_use]
    pub fn stamp(&self, index: usize, facets: LocationFacets) -> LocationFacets {
        // Every record the coordinate locates carries its own copy of a timestamp or text.
        match self {
            CoordinateFacets::Levels(levels) => match levels.get(index) {
                Some(Some(level)) => facets.with_level(level),
                Some(None) | None => facets,
            },
            CoordinateFacets::Times { key, times } => match times.get(index) {
                Some(Some(time)) => facets.with_time(key, time.clone()),
                Some(None) | None => facets,
            },
            CoordinateFacets::Numbers { key, numbers } => match numbers.get(index) {
                Some(number) => facets.with_number(key, *number),
                None => facets,
            },
            CoordinateFacets::Texts { key, texts } => match texts.get(index) {
                Some(text) if !text.is_empty() => facets.with_value(key, Value::String(text.clone())),
                Some(_) | None => facets,
            },
        }
    }
}
