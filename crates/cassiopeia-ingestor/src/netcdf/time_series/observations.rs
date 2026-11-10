use crate::{
    error::IngestorError,
    location_record::{
        facets::LocationFacets,
        longitude::normalize_longitude,
        parameter_name::ParameterName,
        record::{LATITUDE, LONGITUDE, location_record},
    },
    netcdf::{
        cf_attribute::CfAttribute,
        coordinate_facets::{CoordinateFacets, CoordinateKey},
        coordinate_role::CoordinateRole,
        decode::Decode,
        error::NetCdfIngestError,
        record_keys::{RecordKey, ensure_distinct},
        referenced_variables::referenced_variables,
        time_series::{
            layout::{Observation, ObservationLayout},
            stations::TimeSeriesCoordinates,
        },
        value_kind::{ValueKind, value_dimensions},
    },
    record_batches::RecordBatches,
};
use cassiopeia_ir::record::Record;
use netcdf_reader::{NcFile, NcGroup, NcVariable};

/// Emits one location record per observation of a CF `timeSeries` file (CF 1.11 §9, Appendix H.2).
///
/// Each record carries its station's latitude and longitude, the observation's `time`, the station's
/// other coordinates (its level, identifier and name), the text variables of the observation, such as
/// the per-observation station name ERDDAP writes or the data mode flags of `OceanSITES`, and one column
/// per numeric data variable. A data variable is a variable that is not a coordinate, a ragged
/// array's count or index, or a bounds or grid mapping variable, and that varies along the element
/// dimension. Station files are small, so every variable is read whole. An observation whose time is
/// masked is padding (the incomplete layout fills unused slots) and is skipped, as is one where every
/// numeric value is masked.
///
/// # Errors
///
/// Returns [`IngestorError::NetCdf`] when the coordinates or layout cannot be resolved or a variable
/// cannot be read, and [`IngestorError::ChannelClosed`] when the downstream channel has closed.
pub fn ingest_time_series(file: &NcFile, batches: &mut RecordBatches<'_>) -> Result<(), IngestorError> {
    let root = file.root_group().map_err(NetCdfIngestError::from)?;
    let series = TimeSeries::resolve(file, root)?;
    series.read(file)?.emit(&series.layout, batches)
}

/// A `timeSeries` file resolved from its metadata: where its stations and observations are, and which
/// variables its records carry under which keys.
struct TimeSeries<'f> {
    /// Where each observation is along the instance and element dimensions.
    layout: ObservationLayout<'f>,
    /// The station latitude.
    latitude: &'f NcVariable,
    /// The station longitude.
    longitude: &'f NcVariable,
    /// The observation time.
    time: &'f NcVariable,
    /// The key the observation time is emitted under.
    time_key: CoordinateKey,
    /// The other coordinates and the text observation variables the layout places, with their keys.
    facets: Vec<(&'f NcVariable, CoordinateKey)>,
    /// The numeric data variables.
    columns: Vec<&'f NcVariable>,
}

impl<'f> TimeSeries<'f> {
    /// Finds the coordinates, the layout and the data variables of the root group's time series.
    fn resolve(file: &NcFile, root: &'f NcGroup) -> Result<TimeSeries<'f>, NetCdfIngestError> {
        let coordinates = TimeSeriesCoordinates::find(root)?;
        let layout = ObservationLayout::detect(file, root, &coordinates)?;
        let DataVariables { columns, texts } = DataVariables::select(root, &coordinates, &layout);
        if columns.is_empty() {
            return Err(NetCdfIngestError::NoRecordVariables);
        }

        let TimeSeriesCoordinates {
            latitude,
            longitude,
            time,
            facets,
        } = coordinates;
        let texts = texts
            .into_iter()
            .map(|variable| (variable, CoordinateKey::Named(ParameterName::from(variable.name()))));
        let facets = facets.into_iter().filter(|(variable, _)| placed(&layout, variable)).chain(texts).collect();
        let series = TimeSeries {
            layout,
            latitude,
            longitude,
            time,
            time_key: CoordinateKey::of(root, time, CoordinateRole::Time),
            facets,
            columns,
        };
        series.ensure_distinct_keys()?;
        Ok(series)
    }

    /// Fails when two of the records' keys coincide.
    fn ensure_distinct_keys(&self) -> Result<(), NetCdfIngestError> {
        let position = [RecordKey::new(LATITUDE, self.latitude.name()), RecordKey::new(LONGITUDE, self.longitude.name())];
        let facets = self.facets.iter().flat_map(|(variable, key)| key.record_keys(variable));
        let columns = self.columns.iter().map(|column| RecordKey::new(column.name(), column.name()));
        ensure_distinct(position.into_iter().chain(self.time_key.record_keys(self.time)).chain(facets).chain(columns))
    }

    /// Reads every variable the records are built from, whole.
    fn read(&self, file: &NcFile) -> Result<ReadTimeSeries<'_>, NetCdfIngestError> {
        let longitudes = read_location(file, self.longitude, &self.layout)?;
        let facets = self
            .facets
            .iter()
            .map(|(variable, key)| ReadFacet::read(file, variable, key))
            .collect::<Result<_, _>>()?;
        let columns = self
            .columns
            .iter()
            .map(|variable| {
                Ok((
                    ParameterName::from(variable.name()),
                    ReadValues {
                        variable,
                        values: Decode::MaskAndUnpack.read(file, variable)?,
                    },
                ))
            })
            .collect::<Result<_, NetCdfIngestError>>()?;
        Ok(ReadTimeSeries {
            latitude: read_location(file, self.latitude, &self.layout)?,
            longitude: ReadValues {
                variable: longitudes.variable,
                values: longitudes.values.into_iter().map(normalize_longitude).collect(),
            },
            time: ReadFacet::read(file, self.time, &self.time_key)?,
            facets,
            columns,
        })
    }
}

/// The variables a time series' records carry besides its coordinates.
struct DataVariables<'f> {
    /// The numeric data variables, each a column of the records.
    columns: Vec<&'f NcVariable>,
    /// The text variables of the observations, each emitted under its own name.
    texts: Vec<&'f NcVariable>,
}

impl<'f> DataVariables<'f> {
    /// The variables that are not coordinates or auxiliary variables and vary along the element
    /// dimension. One the layout cannot place, or holding a user-defined type, is skipped with a
    /// warning.
    fn select(root: &'f NcGroup, coordinates: &TimeSeriesCoordinates<'f>, layout: &ObservationLayout<'f>) -> DataVariables<'f> {
        let referenced = referenced_variables(root);
        let candidates = root.variables.iter().filter(|variable| {
            !variable.is_coordinate_variable()
                && !coordinates.contains(variable)
                && !referenced.contains(variable.name())
                && CfAttribute::SampleDimension.text(variable.attributes()).is_none()
                && CfAttribute::InstanceDimension.text(variable.attributes()).is_none()
                && layout.varies_by_observation(value_dimensions(variable))
        });
        let mut selected = DataVariables {
            columns: Vec::new(),
            texts: Vec::new(),
        };
        for variable in candidates {
            if !layout.places(value_dimensions(variable)) {
                tracing::warn!(
                    variable = variable.name(),
                    "NetCDF timeSeries variable skipped: it has a dimension outside the station and observation dimensions"
                );
                continue;
            }
            match ValueKind::of(variable.dtype()) {
                ValueKind::Numeric => selected.columns.push(variable),
                ValueKind::Text => selected.texts.push(variable),
                ValueKind::UserDefined => tracing::warn!(variable = variable.name(), "NetCDF timeSeries variable skipped: it holds a user-defined type"),
            }
        }
        selected
    }
}

/// A numeric variable read whole, with masked values as `NaN`.
struct ReadValues<'k> {
    /// The variable, whose dimensions locate each observation's value.
    variable: &'k NcVariable,
    /// Every value, flattened in row-major order.
    values: Vec<f64>,
}

impl ReadValues<'_> {
    /// The value for `observation`, or `None` when it is missing.
    fn at(&self, layout: &ObservationLayout<'_>, observation: Observation) -> Option<f64> {
        let index = layout.flat_index(self.variable.dimensions(), observation)?;
        self.values.get(index).copied().filter(|value| !value.is_nan())
    }
}

/// A coordinate or text variable read whole and decoded into record facets.
struct ReadFacet<'k> {
    /// The variable, whose value dimensions locate each observation's value.
    variable: &'k NcVariable,
    /// Its decoded values.
    facets: CoordinateFacets<'k>,
}

impl<'k> ReadFacet<'k> {
    /// Reads `variable` whole, masked and unpacked, to be emitted under `key`.
    fn read(file: &NcFile, variable: &'k NcVariable, key: &'k CoordinateKey) -> Result<ReadFacet<'k>, NetCdfIngestError> {
        Ok(ReadFacet {
            variable,
            facets: CoordinateFacets::read(file, variable, key, Decode::MaskAndUnpack)?,
        })
    }

    /// The position of `observation`'s value, or `None` when the layout does not place the variable.
    fn index(&self, layout: &ObservationLayout<'_>, observation: Observation) -> Option<usize> {
        layout.flat_index(value_dimensions(self.variable), observation)
    }
}

/// Everything a time series' records are built from, read whole.
struct ReadTimeSeries<'k> {
    /// The station latitudes.
    latitude: ReadValues<'k>,
    /// The station longitudes, folded into `[-180, 180)`.
    longitude: ReadValues<'k>,
    /// The observation times.
    time: ReadFacet<'k>,
    /// The other coordinates and the text observation variables.
    facets: Vec<ReadFacet<'k>>,
    /// The numeric data variables, with the names their columns carry.
    columns: Vec<(ParameterName, ReadValues<'k>)>,
}

impl ReadTimeSeries<'_> {
    /// Emits the record of every observation that has a time, a position and a value.
    fn emit(&self, layout: &ObservationLayout<'_>, batches: &mut RecordBatches<'_>) -> Result<(), IngestorError> {
        layout
            .observations()
            .iter()
            .filter_map(|&observation| self.record(layout, observation))
            .try_for_each(|record| batches.push(record))
    }

    /// The record of one observation, or `None` when its time or position is missing or every value is.
    fn record(&self, layout: &ObservationLayout<'_>, observation: Observation) -> Option<Record> {
        let time_index = self.time.index(layout, observation).filter(|index| self.time.facets.has_value(*index))?;
        let latitude = self.latitude.at(layout, observation)?;
        let longitude = self.longitude.at(layout, observation)?;
        let facets = self
            .facets
            .iter()
            .fold(self.time.facets.stamp(time_index, LocationFacets::default()), |facets, facet| {
                match facet.index(layout, observation) {
                    Some(index) => facet.facets.stamp(index, facets),
                    None => facets,
                }
            });
        let values = self
            .columns
            .iter()
            .filter_map(|(name, column)| column.at(layout, observation).map(|value| (name, value)));
        location_record(latitude, longitude, &facets, values)
    }
}

/// Whether the layout places a coordinate, warning when it does not and the coordinate is skipped.
fn placed(layout: &ObservationLayout<'_>, variable: &NcVariable) -> bool {
    let placed = layout.places(value_dimensions(variable));
    if !placed {
        tracing::warn!(
            variable = variable.name(),
            "NetCDF timeSeries coordinate skipped: it has a dimension outside the station and observation dimensions"
        );
    }
    placed
}

/// Reads a station latitude or longitude whole, refusing one the layout cannot place.
fn read_location<'k>(file: &NcFile, variable: &'k NcVariable, layout: &ObservationLayout<'_>) -> Result<ReadValues<'k>, NetCdfIngestError> {
    if !layout.places(variable.dimensions()) {
        return Err(NetCdfIngestError::UnrecognizedTimeSeriesLayout);
    }
    Ok(ReadValues {
        variable,
        values: Decode::MaskAndUnpack.read(file, variable)?,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        error::IngestorError,
        netcdf::{
            coordinate_role::CoordinateRole,
            error::NetCdfIngestError,
            fixture::{annotate, coordinate, filled_variable, ingest, text, variable, write},
        },
    };
    use cassiopeia_ir::record::Record;
    use netcdf_writer::{DimensionId, NC_FILL_INT, NcFileBuilder, NcWriteFormat};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};
    use temp_dir::TempDir;

    const FILL: f64 = -999.0;
    const TIME: &[(&str, &str)] = &[("standard_name", "time"), ("units", "hours since 2026-01-01")];
    const LATITUDE: &[(&str, &str)] = &[("standard_name", "latitude"), ("units", "degrees_north")];
    const LONGITUDE: &[(&str, &str)] = &[("standard_name", "longitude"), ("units", "degrees_east")];

    /// A builder declaring the given CF feature type.
    fn feature(feature_type: &str) -> NcFileBuilder {
        let mut builder = NcFileBuilder::new();
        builder.add_attribute("featureType", text(feature_type)).unwrap();
        builder
    }

    /// Two stations along `station`: Ljubljana at 46 N 14 E and Maribor at 47 N 15 E, named by a
    /// `timeseries_id` character variable.
    fn stations(builder: &mut NcFileBuilder, station: DimensionId) {
        variable(builder, "lat", &[station], &[46.0, 47.0], LATITUDE);
        variable(builder, "lon", &[station], &[14.0, 15.0], LONGITUDE);
        let name_length = builder.add_dimension("name_strlen", 3).unwrap();
        let name = builder.add_char_variable("station_name", &[station, name_length]).unwrap();
        annotate(builder, name, &[("cf_role", "timeseries_id")]);
        builder.write_char_variable_strings(name, &["LJU", "MBX"]).unwrap();
    }

    fn ingested(builder: &NcFileBuilder) -> Result<Vec<Record>, IngestorError> {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("stations.nc");
        write(builder, &path, NcWriteFormat::Classic);
        ingest(&path)
    }

    fn data(records: &[Record]) -> Vec<Map<String, Value>> {
        records.iter().map(|record| record.data().clone()).collect()
    }

    /// The station name, time and temperature of each record.
    fn readings(records: &[Record]) -> Vec<(Value, Value, Value)> {
        records
            .iter()
            .map(|record| {
                let field = |key: &str| record.data().get(key).cloned().unwrap_or(Value::Null);
                (field("station_name"), field("time"), field("temp"))
            })
            .collect()
    }

    /// The order of the dimensions of an orthogonal fixture's data variable.
    enum Order {
        StationTime,
        TimeStation,
    }

    /// Two stations observed at the same three times, the orthogonal multidimensional layout.
    fn orthogonal(order: &Order) -> NcFileBuilder {
        let mut builder = feature("timeSeries");
        let station = builder.add_dimension("station", 2).unwrap();
        stations(&mut builder, station);
        let time = coordinate(&mut builder, "time", &[0.0, 1.0, 2.0], TIME);
        let coordinates = [("coordinates", "lat lon station_name")];
        match order {
            Order::StationTime => variable(&mut builder, "temp", &[station, time], &[10.0, 11.0, 12.0, 20.0, 21.0, 22.0], &coordinates),
            Order::TimeStation => variable(&mut builder, "temp", &[time, station], &[10.0, 20.0, 11.0, 21.0, 12.0, 22.0], &coordinates),
        };
        builder
    }

    /// Two stations' observations stored one after the other, counted by `row_size`.
    fn contiguous(row_sizes: &[i32], observations: u64) -> NcFileBuilder {
        contiguous_along(row_sizes, observations).0
    }

    /// The contiguous fixture, with its observation dimension.
    fn contiguous_along(row_sizes: &[i32], observations: u64) -> (NcFileBuilder, DimensionId) {
        let mut builder = feature("timeSeries");
        let station = builder.add_dimension("station", 2).unwrap();
        stations(&mut builder, station);
        let obs = builder.add_dimension("obs", observations).unwrap();
        let counts = builder.add_variable::<i32>("row_size", &[station]).unwrap();
        annotate(&mut builder, counts, &[("sample_dimension", "obs")]);
        builder.write_variable(counts, row_sizes).unwrap();
        variable(&mut builder, "time", &[obs], &[0.0, 1.0, 0.0, 1.0, 2.0], TIME);
        variable(
            &mut builder,
            "temp",
            &[obs],
            &[10.0, 11.0, 20.0, 21.0, 22.0],
            &[("coordinates", "time lat lon station_name")],
        );
        (builder, obs)
    }

    #[test]
    fn a_single_station_becomes_one_record_per_time_with_its_identifier() {
        let mut builder = feature("timeSeries");
        let time = coordinate(&mut builder, "time", &[0.0, 1.0, 2.0], TIME);
        variable(&mut builder, "lat", &[], &[46.05], LATITUDE);
        variable(&mut builder, "lon", &[], &[14.5], LONGITUDE);
        let id_length = builder.add_dimension("id_strlen", 3).unwrap();
        let id = builder.add_char_variable("station_id", &[id_length]).unwrap();
        annotate(&mut builder, id, &[("cf_role", "timeseries_id")]);
        builder.write_char_variable_strings(id, &["LJU"]).unwrap();
        variable(&mut builder, "temp", &[time], &[10.0, 11.0, 12.0], &[("coordinates", "lat lon")]);

        let records = ingested(&builder).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(
            Value::Object(records[1].data().clone()),
            json!({"latitude": 46.05, "longitude": 14.5, "time": "2026-01-01T01:00:00+00:00", "station_id": "LJU", "temp": 11.0})
        );
        assert!(records.iter().all(|record| record.data().get("station_id") == Some(&json!("LJU"))));
    }

    #[test]
    fn an_orthogonal_station_by_time_layout_gives_each_station_its_own_position_and_name() {
        let records = ingested(&orthogonal(&Order::StationTime)).unwrap();
        assert_eq!(records.len(), 6);
        assert_eq!(
            Value::Object(records[0].data().clone()),
            json!({"latitude": 46.0, "longitude": 14.0, "time": "2026-01-01T00:00:00+00:00", "station_name": "LJU", "temp": 10.0})
        );
        assert_eq!(
            Value::Object(records[5].data().clone()),
            json!({"latitude": 47.0, "longitude": 15.0, "time": "2026-01-01T02:00:00+00:00", "station_name": "MBX", "temp": 22.0})
        );
    }

    #[test]
    fn an_orthogonal_time_by_station_layout_gives_the_same_records() {
        let station_time = ingested(&orthogonal(&Order::StationTime)).unwrap();
        let time_station = ingested(&orthogonal(&Order::TimeStation)).unwrap();
        assert_eq!(data(&time_station), data(&station_time));
    }

    #[test]
    fn an_incomplete_layout_skips_the_fill_padded_slots() {
        let mut builder = feature("timeSeries");
        let station = builder.add_dimension("station", 2).unwrap();
        stations(&mut builder, station);
        let obs = builder.add_dimension("obs", 3).unwrap();
        filled_variable(&mut builder, "time", &[station, obs], &[0.0, 1.0, 2.0, 5.0, 6.0, FILL], FILL, TIME);
        filled_variable(
            &mut builder,
            "temp",
            &[station, obs],
            &[10.0, 11.0, 12.0, 20.0, 21.0, FILL],
            FILL,
            &[("coordinates", "time lat lon station_name")],
        );

        let records = ingested(&builder).unwrap();
        assert_eq!(
            readings(&records),
            vec![
                (json!("LJU"), json!("2026-01-01T00:00:00+00:00"), json!(10.0)),
                (json!("LJU"), json!("2026-01-01T01:00:00+00:00"), json!(11.0)),
                (json!("LJU"), json!("2026-01-01T02:00:00+00:00"), json!(12.0)),
                (json!("MBX"), json!("2026-01-01T05:00:00+00:00"), json!(20.0)),
                (json!("MBX"), json!("2026-01-01T06:00:00+00:00"), json!(21.0)),
            ]
        );
    }

    #[test]
    fn a_contiguous_ragged_array_assigns_each_run_of_observations_to_its_station() {
        let records = ingested(&contiguous(&[2, 3], 5)).unwrap();
        assert_eq!(
            readings(&records),
            vec![
                (json!("LJU"), json!("2026-01-01T00:00:00+00:00"), json!(10.0)),
                (json!("LJU"), json!("2026-01-01T01:00:00+00:00"), json!(11.0)),
                (json!("MBX"), json!("2026-01-01T00:00:00+00:00"), json!(20.0)),
                (json!("MBX"), json!("2026-01-01T01:00:00+00:00"), json!(21.0)),
                (json!("MBX"), json!("2026-01-01T02:00:00+00:00"), json!(22.0)),
            ]
        );
        assert_eq!(records[2].data().get("latitude"), Some(&json!(47.0)));
    }

    #[test]
    fn an_indexed_ragged_array_assigns_each_observation_to_its_indexed_station() {
        let mut builder = feature("timeSeries");
        let station = builder.add_dimension("station", 2).unwrap();
        stations(&mut builder, station);
        let obs = builder.add_dimension("obs", 3).unwrap();
        let index = builder.add_variable::<i32>("stationIndex", &[obs]).unwrap();
        annotate(&mut builder, index, &[("instance_dimension", "station")]);
        builder.write_variable(index, &[1, 0, 1]).unwrap();
        variable(&mut builder, "time", &[obs], &[0.0, 0.0, 1.0], TIME);
        variable(
            &mut builder,
            "temp",
            &[obs],
            &[20.0, 10.0, 21.0],
            &[("coordinates", "time lat lon station_name")],
        );

        let records = ingested(&builder).unwrap();
        assert_eq!(
            readings(&records),
            vec![
                (json!("MBX"), json!("2026-01-01T00:00:00+00:00"), json!(20.0)),
                (json!("LJU"), json!("2026-01-01T00:00:00+00:00"), json!(10.0)),
                (json!("MBX"), json!("2026-01-01T01:00:00+00:00"), json!(21.0)),
            ]
        );
        assert_eq!(records[0].data().get("longitude"), Some(&json!(15.0)));
    }

    #[test]
    fn row_sizes_running_past_the_observation_dimension_are_an_invalid_ragged_array() {
        let error = ingested(&contiguous(&[2, 4], 5)).unwrap_err();
        assert!(
            matches!(&error, IngestorError::NetCdf(NetCdfIngestError::InvalidRaggedArray { variable }) if variable == "row_size"),
            "{error:?}"
        );
    }

    #[test]
    fn a_text_observation_variable_is_emitted_under_its_own_name_and_an_empty_one_is_omitted() {
        let (mut builder, obs) = contiguous_along(&[2, 3], 5);
        let flag_length = builder.add_dimension("flag_strlen", 1).unwrap();
        let flag = builder.add_char_variable("temp_dm", &[obs, flag_length]).unwrap();
        builder.write_char_variable_strings(flag, &["D", "", "R", "R", "D"]).unwrap();

        let records = ingested(&builder).unwrap();
        let flags: Vec<Option<&Value>> = records.iter().map(|record| record.data().get("temp_dm")).collect();
        assert_eq!(flags, vec![Some(&json!("D")), None, Some(&json!("R")), Some(&json!("R")), Some(&json!("D"))]);
        assert_eq!(records[1].data().get("temp"), Some(&json!(11.0)));
    }

    #[test]
    fn an_oceansites_station_with_length_one_position_and_height_dimensions_is_one_series() {
        let mut builder = feature("timeSeries");
        let time = coordinate(&mut builder, "TIME", &[0.0, 1.0, 2.0], TIME);
        let height = coordinate(
            &mut builder,
            "HEIGHT",
            &[4.0],
            &[("standard_name", "height"), ("units", "meters"), ("positive", "up"), ("axis", "Z")],
        );
        coordinate(&mut builder, "LATITUDE", &[0.0], LATITUDE);
        coordinate(&mut builder, "LONGITUDE", &[-140.0], LONGITUDE);
        variable(&mut builder, "AIRT", &[time, height], &[24.5, 24.6, 24.7], &[("units", "degree_Celsius")]);

        let records = ingested(&builder).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(
            Value::Object(records[2].data().clone()),
            json!({"latitude": 0.0, "longitude": -140.0, "time": "2026-01-01T02:00:00+00:00", "level_type": "height_above_ground", "level": 4, "AIRT": 24.7})
        );
    }

    #[test]
    fn a_mooring_depth_under_a_sea_water_temperature_and_its_unnamed_flag_is_a_depth_below_the_sea() {
        let mut builder = feature("timeSeries");
        let time = coordinate(&mut builder, "TIME", &[0.0, 1.0, 2.0], TIME);
        let depth = coordinate(
            &mut builder,
            "DEPTH",
            &[1.5],
            &[("standard_name", "depth"), ("units", "meters"), ("positive", "down"), ("axis", "Z")],
        );
        coordinate(&mut builder, "LATITUDE", &[0.0], LATITUDE);
        coordinate(&mut builder, "LONGITUDE", &[-140.0], LONGITUDE);
        variable(
            &mut builder,
            "TEMP",
            &[time, depth],
            &[24.5, 24.6, 24.7],
            &[("standard_name", "sea_water_temperature"), ("units", "degree_Celsius")],
        );
        variable(&mut builder, "TEMP_QC", &[time, depth], &[1.0, 1.0, 4.0], &[("long_name", "quality flag")]);

        let records = ingested(&builder).unwrap();
        assert_eq!(
            Value::Object(records[2].data().clone()),
            json!({
                "latitude": 0.0,
                "longitude": -140.0,
                "time": "2026-01-01T02:00:00+00:00",
                "level_type": "depth_below_sea",
                "level": 1.5,
                "TEMP": 24.7,
                "TEMP_QC": 4.0
            })
        );
    }

    #[test]
    fn a_ragged_file_whose_counts_lack_their_sample_dimension_is_an_unrecognized_layout() {
        let mut builder = feature("timeSeries");
        let station = builder.add_dimension("station", 2).unwrap();
        stations(&mut builder, station);
        let obs = builder.add_dimension("obs", 5).unwrap();
        let counts = builder.add_variable::<i32>("row_size", &[station]).unwrap();
        builder.write_variable(counts, &[2, 3]).unwrap();
        variable(&mut builder, "time", &[obs], &[0.0, 1.0, 0.0, 1.0, 2.0], TIME);
        variable(
            &mut builder,
            "temp",
            &[obs],
            &[10.0, 11.0, 20.0, 21.0, 22.0],
            &[("coordinates", "time lat lon")],
        );

        let error = ingested(&builder).unwrap_err();
        assert!(
            matches!(error, IngestorError::NetCdf(NetCdfIngestError::UnrecognizedTimeSeriesLayout)),
            "{error:?}"
        );
    }

    #[test]
    fn elements_past_the_last_station_count_belong_to_no_station() {
        let records = ingested(&contiguous(&[2, 2], 5)).unwrap();
        assert_eq!(records.len(), 4);
        assert_eq!(records[3].data().get("temp"), Some(&json!(21.0)));
    }

    #[test]
    fn a_station_altitude_becomes_the_level() {
        let mut builder = feature("timeSeries");
        let station = builder.add_dimension("station", 2).unwrap();
        stations(&mut builder, station);
        let time = coordinate(&mut builder, "time", &[0.0], TIME);
        variable(
            &mut builder,
            "alt",
            &[station],
            &[295.0, 275.0],
            &[("standard_name", "altitude"), ("units", "m")],
        );
        variable(
            &mut builder,
            "temp",
            &[station, time],
            &[10.0, 20.0],
            &[("coordinates", "lat lon alt station_name")],
        );

        let records = ingested(&builder).unwrap();
        assert_eq!(records[0].data().get("level_type"), Some(&json!("altitude_above_mean_sea")));
        assert_eq!(records[0].data().get("level"), Some(&json!(295)));
        assert_eq!(records[1].data().get("level"), Some(&json!(275)));
    }

    #[test]
    fn a_trajectory_is_an_unsupported_feature_type() {
        let mut builder = feature("trajectory");
        let time = coordinate(&mut builder, "time", &[0.0], TIME);
        variable(&mut builder, "temp", &[time], &[10.0], &[]);

        let error = ingested(&builder).unwrap_err();
        assert!(matches!(error, IngestorError::NetCdf(NetCdfIngestError::UnsupportedFeatureType { .. })));
    }

    #[test]
    fn a_time_series_without_a_latitude_is_missing_that_coordinate() {
        let mut builder = feature("timeSeries");
        let time = coordinate(&mut builder, "time", &[0.0], TIME);
        variable(&mut builder, "lat", &[], &[46.0], &[]);
        variable(&mut builder, "lon", &[], &[14.0], LONGITUDE);
        variable(&mut builder, "temp", &[time], &[10.0], &[("coordinates", "lat lon")]);

        let error = ingested(&builder).unwrap_err();
        assert!(matches!(
            error,
            IngestorError::NetCdf(NetCdfIngestError::MissingTimeSeriesCoordinate {
                role: CoordinateRole::Latitude
            })
        ));
    }

    #[test]
    fn a_masked_value_is_omitted_and_an_observation_masked_everywhere_yields_no_record() {
        let mut builder = feature("timeSeries");
        let time = coordinate(&mut builder, "time", &[0.0, 1.0, 2.0], TIME);
        variable(&mut builder, "lat", &[], &[46.0], LATITUDE);
        variable(&mut builder, "lon", &[], &[14.0], LONGITUDE);
        let coordinates = [("coordinates", "lat lon")];
        filled_variable(&mut builder, "temp", &[time], &[FILL, 11.0, FILL], FILL, &coordinates);
        filled_variable(&mut builder, "rh", &[time], &[60.0, 61.0, FILL], FILL, &coordinates);

        let records = ingested(&builder).unwrap();
        assert_eq!(records.len(), 2);
        assert!(!records[0].data().contains_key("temp"));
        assert_eq!(records[0].data().get("rh"), Some(&json!(60.0)));
        assert_eq!(records[1].data().get("temp"), Some(&json!(11.0)));
    }

    #[test]
    fn an_unset_station_identifier_without_a_fill_value_is_omitted() {
        let mut builder = feature("timeSeries");
        let station = builder.add_dimension("station", 1).unwrap();
        let identifier = builder.add_variable::<i32>("station", &[station]).unwrap();
        annotate(&mut builder, identifier, &[("cf_role", "timeseries_id")]);
        builder.write_variable(identifier, &[NC_FILL_INT]).unwrap();
        let time = coordinate(&mut builder, "time", &[0.0], TIME);
        variable(&mut builder, "lat", &[station], &[46.0], LATITUDE);
        variable(&mut builder, "lon", &[station], &[14.0], LONGITUDE);
        variable(&mut builder, "temp", &[station, time], &[10.0], &[("coordinates", "lat lon")]);

        let records = ingested(&builder).unwrap();
        assert_eq!(records.len(), 1);
        assert!(!records[0].data().contains_key("station"));
        assert_eq!(records[0].data().get("temp"), Some(&json!(10.0)));
    }
}
