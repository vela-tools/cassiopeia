use crate::{
    location_record::{
        facets::{LEVEL, LEVEL_TYPE, LocationFacets},
        level::Level,
        parameter_name::ParameterName,
    },
    netcdf::{
        coordinate_role::CoordinateRole,
        decode::Decode,
        decoded_time::DecodedTime,
        depth_surface::DepthSurface,
        error::NetCdfIngestError,
        nonexistent_dates::{NonexistentDateLog, NonexistentDates},
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
    /// variable carries CF time units (whatever its role), and plain numbers otherwise. Timestamps whose
    /// dates the Gregorian calendar does not have, which only a model calendar states, are kept as they
    /// are and recorded in `nonexistent`.
    ///
    /// # Errors
    ///
    /// Returns [`NetCdfIngestError`] when the variable cannot be read, holds a user-defined type, or
    /// carries time units that cannot be decoded.
    pub fn read(
        file: &NcFile,
        variable: &NcVariable,
        key: &'k CoordinateKey,
        decode: Decode,
        nonexistent: &mut NonexistentDateLog,
    ) -> Result<CoordinateFacets<'k>, NetCdfIngestError> {
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
                        let decoded = numbers
                            .into_iter()
                            .map(|value| if value.is_nan() { Ok(None) } else { axis.decode(value).map(Some) })
                            .collect::<Result<Vec<_>, _>>()?;
                        if let Some(dates) = NonexistentDates::among(variable, axis.calendar(), &decoded) {
                            nonexistent.record(dates);
                        }
                        let times = decoded.into_iter().map(|time| time.map(DecodedTime::into_timestamp)).collect();
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

#[cfg(test)]
mod tests {
    use crate::{
        location_record::parameter_name::ParameterName,
        netcdf::{
            calendar::Calendar,
            coordinate_facets::{CoordinateFacets, CoordinateKey},
            decode::Decode,
            fixture::{filled_variable, write},
            nonexistent_dates::NonexistentDateLog,
            variable_name::VariableName,
        },
    };
    use netcdf_reader::NcFile;
    use netcdf_writer::{NcFileBuilder, NcWriteFormat};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU64, path::PathBuf};
    use temp_dir::TempDir;

    const FILL: f64 = -999.0;

    /// Writes a file holding one variable `time` over `values`, carrying `attributes`, where `FILL`
    /// marks a missing value.
    fn written(dir: &TempDir, values: &[f64], attributes: &[(&str, &str)]) -> PathBuf {
        let mut builder = NcFileBuilder::new();
        let dimension = builder.add_dimension("obs", u64::try_from(values.len()).unwrap()).unwrap();
        filled_variable(&mut builder, "time", &[dimension], values, FILL, attributes);
        let path = dir.path().join("times.nc");
        write(&builder, &path, NcWriteFormat::Classic);
        path
    }

    /// The timestamps the variable `time` of a file written by [`written`] reads as.
    fn timestamps(values: &[f64], attributes: &[(&str, &str)], nonexistent: &mut NonexistentDateLog) -> Vec<Option<String>> {
        let dir = TempDir::new().unwrap();
        let file = NcFile::open(written(&dir, values, attributes)).unwrap();
        let key = CoordinateKey::Named(ParameterName::from("time"));
        match CoordinateFacets::read(&file, file.variable("time").unwrap(), &key, Decode::MaskAndUnpack, nonexistent).unwrap() {
            CoordinateFacets::Times { times, .. } => times,
            CoordinateFacets::Levels(_) | CoordinateFacets::Numbers { .. } | CoordinateFacets::Texts { .. } => panic!("expected timestamps"),
        }
    }

    #[test]
    fn a_julian_time_reads_as_the_gregorian_dates_of_its_days_and_logs_nothing() {
        let mut nonexistent = NonexistentDateLog::default();
        let times = timestamps(&[0.0, FILL], &[("units", "days since 2026-01-01"), ("calendar", "julian")], &mut nonexistent);

        assert_eq!(times, [Some("2026-01-14T00:00:00+00:00".to_owned()), None]);
        assert_eq!(nonexistent.variables(), []);
    }

    #[test]
    fn a_360_day_time_keeps_its_labels_and_logs_those_the_gregorian_calendar_lacks_skipping_missing_values() {
        let mut nonexistent = NonexistentDateLog::default();
        let times = timestamps(
            &[58.0, FILL, 59.0, 60.0],
            &[("units", "days since 2026-01-01"), ("calendar", "360_day")],
            &mut nonexistent,
        );

        assert_eq!(
            times,
            [
                Some("2026-02-29T00:00:00+00:00".to_owned()),
                None,
                Some("2026-02-30T00:00:00+00:00".to_owned()),
                Some("2026-03-01T00:00:00+00:00".to_owned()),
            ]
        );
        let logged = nonexistent.variables();
        assert_eq!(logged.len(), 1);
        assert_eq!(
            (&logged[0].variable, logged[0].calendar, logged[0].first.as_str(), logged[0].count),
            (
                &VariableName::from("time"),
                Calendar::Day360,
                "2026-02-29T00:00:00+00:00",
                NonZeroU64::new(2).unwrap()
            )
        );
    }

    #[test]
    fn a_number_without_time_units_reads_as_a_number_and_logs_nothing() {
        let dir = TempDir::new().unwrap();
        let file = NcFile::open(written(&dir, &[850.0], &[("units", "hPa")])).unwrap();
        let key = CoordinateKey::Named(ParameterName::from("time"));
        let mut nonexistent = NonexistentDateLog::default();

        let facets = CoordinateFacets::read(&file, file.variable("time").unwrap(), &key, Decode::MaskAndUnpack, &mut nonexistent).unwrap();
        assert!(matches!(facets, CoordinateFacets::Numbers { numbers, .. } if numbers == [850.0]));
        assert_eq!(nonexistent.variables(), []);
    }
}
