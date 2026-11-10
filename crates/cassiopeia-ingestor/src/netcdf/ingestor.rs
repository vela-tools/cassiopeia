use crate::{
    error::IngestorError,
    ingestor::Ingestor,
    netcdf::{
        error::NetCdfIngestError,
        feature_type::FeatureType,
        grid::{plane::ingest_grid_group, variables::grid_groups},
        time_series::observations::ingest_time_series,
    },
    record_batches::RecordBatches,
};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
use derive_more::Debug;
use netcdf_reader::NcFile;

/// Ingestor for `NetCDF` files: classic CDF-1, CDF-2 and CDF-5, and `NetCDF-4` on HDF5.
///
/// A gridded file (no global `featureType`) pivots its data variables into the same wide location
/// records GRIB produces: every non-masked cell of every latitude-longitude plane becomes a record
/// carrying its `latitude` and `longitude`, the plane's `time`, `level_type` and `level`, and one
/// column per data variable sharing the plane's dimensions. A CF `timeSeries` file emits one such
/// record per station observation. Coordinates are recognized by their CF attributes, never their
/// names, so one mapping fits every producer.
#[derive(Debug)]
pub struct NetCdfIngestor {
    #[debug(skip)]
    file: Option<NcFile>,
    batch_size: usize,
}

impl NetCdfIngestor {
    /// Creates a `NetCDF` ingestor from a profiled payload, opening the file and reading its metadata.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is an in-memory byte buffer rather than a file, or
    /// the file cannot be opened as `NetCDF`.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize) -> Result<NetCdfIngestor, IngestorError> {
        let path = match payload.into_payload() {
            CollectedPayload::File(file) => file.into_path(),
            CollectedPayload::Bytes(_) => return Err(NetCdfIngestError::RequiresFile.into()),
        };
        let file = NcFile::open(&path).map_err(NetCdfIngestError::from)?;
        Ok(NetCdfIngestor { file: Some(file), batch_size })
    }
}

impl Ingestor for NetCdfIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let file = self.file.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let mut batches = RecordBatches::new(&sender, self.batch_size);
        match FeatureType::of(file.global_attributes().map_err(NetCdfIngestError::from)?)? {
            FeatureType::Grid => {
                let groups = grid_groups(file.root_group().map_err(NetCdfIngestError::from)?)?;
                groups.iter().try_for_each(|group| ingest_grid_group(&file, group, &mut batches))?;
            }
            FeatureType::TimeSeries => ingest_time_series(&file, &mut batches)?,
        }
        batches.finish()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::IngestorError,
        ingestor::Ingestor,
        netcdf::{
            error::NetCdfIngestError,
            fixture::{coordinate, filled_variable, ingest, ingest_batches, payload, variable, write},
            ingestor::NetCdfIngestor,
        },
    };
    use cassiopeia_common::{channel::ChannelSender, format::DataFormat};
    use cassiopeia_data_profiler::profile::Profile;
    use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
    use cassiopeia_ir::{
        payload::{BytesPayload, CollectedPayload, ProfiledPayload},
        record::Record,
    };
    use mediatype::media_type;
    use netcdf_writer::{DimensionId, NC_FILL_INT, NcAttrValue, NcFileBuilder, NcWriteFormat, VariableId};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};
    use std::{fs, path::PathBuf, sync::mpsc::sync_channel};
    use temp_dir::TempDir;

    const FILL: f64 = -999.0;

    /// The 1 x 2 x 3 time-latitude-longitude axes most grid fixtures share.
    #[derive(Clone, Copy)]
    struct Axes {
        time: DimensionId,
        latitude: DimensionId,
        longitude: DimensionId,
    }

    impl Axes {
        fn grid(self) -> [DimensionId; 3] {
            [self.time, self.latitude, self.longitude]
        }
    }

    fn axes(builder: &mut NcFileBuilder) -> Axes {
        Axes {
            time: coordinate(
                builder,
                "time",
                &[6.0],
                &[("standard_name", "time"), ("units", "hours since 2026-01-01 00:00:00")],
            ),
            latitude: coordinate(builder, "lat", &[46.0, 46.5], &[("units", "degrees_north")]),
            longitude: coordinate(builder, "lon", &[14.0, 14.5, 15.0], &[("units", "degrees_east")]),
        }
    }

    /// The temperature and humidity grid of the basic case, with its axes and two variables.
    fn temperature_and_humidity() -> (NcFileBuilder, Axes, [VariableId; 2]) {
        let mut builder = NcFileBuilder::new();
        let axes = axes(&mut builder);
        let temperature = variable(&mut builder, "t", &axes.grid(), &[280.0, 281.0, 282.0, 283.0, 284.0, 285.0], &[("units", "K")]);
        let humidity = variable(&mut builder, "r", &axes.grid(), &[50.0, 51.0, 52.0, 53.0, 54.0, 55.0], &[("units", "%")]);
        (builder, axes, [temperature, humidity])
    }

    /// A single-cell latitude-longitude grid.
    fn single_cell(builder: &mut NcFileBuilder) -> [DimensionId; 2] {
        [
            coordinate(builder, "lat", &[46.0], &[("units", "degrees_north")]),
            coordinate(builder, "lon", &[14.0], &[("units", "degrees_east")]),
        ]
    }

    fn written(dir: &TempDir, builder: &NcFileBuilder, format: NcWriteFormat) -> PathBuf {
        let path = dir.path().join("grid.nc");
        write(builder, &path, format);
        path
    }

    fn ingested_as(builder: &NcFileBuilder, format: NcWriteFormat) -> Vec<Record> {
        let dir = TempDir::new().unwrap();
        ingest(&written(&dir, builder, format)).unwrap()
    }

    fn ingested(builder: &NcFileBuilder) -> Vec<Record> {
        ingested_as(builder, NcWriteFormat::Classic)
    }

    fn failure(builder: &NcFileBuilder) -> IngestorError {
        let dir = TempDir::new().unwrap();
        ingest(&written(&dir, builder, NcWriteFormat::Classic)).unwrap_err()
    }

    fn data(records: &[Record]) -> Vec<Map<String, Value>> {
        records.iter().map(|record| record.data().clone()).collect()
    }

    #[test]
    fn a_two_by_three_grid_at_one_time_becomes_six_records_with_both_variables() {
        let records = ingested(&temperature_and_humidity().0);

        assert_eq!(records.len(), 6);
        assert!(records.iter().all(|record| record.collection().is_none()));
        let keys: Vec<&str> = records[0].data().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["latitude", "longitude", "time", "t", "r"]);
        assert_eq!(
            Value::Object(records[0].data().clone()),
            json!({"latitude": 46.0, "longitude": 14.0, "time": "2026-01-01T06:00:00+00:00", "t": 280.0, "r": 50.0})
        );
        assert_eq!(
            Value::Object(records[5].data().clone()),
            json!({"latitude": 46.5, "longitude": 15.0, "time": "2026-01-01T06:00:00+00:00", "t": 285.0, "r": 55.0})
        );
    }

    #[test]
    fn a_longitude_east_of_one_eighty_is_folded_west() {
        let mut builder = NcFileBuilder::new();
        let latitude = coordinate(&mut builder, "lat", &[46.0], &[("units", "degrees_north")]);
        let longitude = coordinate(&mut builder, "lon", &[350.0], &[("units", "degrees_east")]);
        variable(&mut builder, "t", &[latitude, longitude], &[280.0], &[]);

        assert_eq!(ingested(&builder)[0].data().get("longitude"), Some(&json!(-10.0)));
    }

    #[test]
    fn a_pressure_dimension_in_pascals_becomes_isobaric_levels_in_hectopascals() {
        let mut builder = NcFileBuilder::new();
        let axes = axes(&mut builder);
        let pressure = coordinate(&mut builder, "plev", &[85_000.0, 50_000.0], &[("units", "Pa"), ("positive", "down")]);
        variable(
            &mut builder,
            "t",
            &[axes.time, pressure, axes.latitude, axes.longitude],
            &[280.0, 281.0, 282.0, 283.0, 284.0, 285.0, 250.0, 251.0, 252.0, 253.0, 254.0, 255.0],
            &[],
        );

        let records = ingested(&builder);
        assert_eq!(records.len(), 12);
        assert!(records.iter().all(|record| record.data().get("level_type") == Some(&json!("isobaric"))));
        assert!(records[..6].iter().all(|record| record.data().get("level") == Some(&json!(850))));
        assert!(records[6..].iter().all(|record| record.data().get("level") == Some(&json!(500))));
        assert_eq!(records[6].data().get("t"), Some(&json!(250.0)));
    }

    #[test]
    fn a_packed_short_is_unpacked_with_its_scale_factor_and_offset() {
        let mut builder = NcFileBuilder::new();
        let cell = single_cell(&mut builder);
        let packed = builder.add_variable::<i16>("t", &cell).unwrap();
        builder
            .add_variable_attribute(packed, "scale_factor", NcAttrValue::Doubles(vec![0.01]))
            .unwrap();
        builder
            .add_variable_attribute(packed, "add_offset", NcAttrValue::Doubles(vec![273.15]))
            .unwrap();
        builder.write_variable(packed, &[500_i16]).unwrap();

        let temperature = ingested(&builder)[0].data().get("t").and_then(Value::as_f64).unwrap();
        assert!((temperature - 278.15).abs() < 1e-9, "unpacked to {temperature}");
    }

    #[test]
    fn an_unwritten_value_of_a_variable_without_a_fill_value_is_omitted() {
        let mut builder = NcFileBuilder::new();
        let latitude = coordinate(&mut builder, "lat", &[46.0, 46.5], &[("units", "degrees_north")]);
        let longitude = coordinate(&mut builder, "lon", &[14.0], &[("units", "degrees_east")]);
        let count = builder.add_variable::<i32>("count", &[latitude, longitude]).unwrap();
        builder.write_variable(count, &[NC_FILL_INT, 7]).unwrap();

        let records = ingested(&builder);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data().get("count"), Some(&json!(7.0)));
    }

    #[test]
    fn a_filled_value_drops_its_column_and_a_cell_filled_everywhere_drops_its_record() {
        let mut builder = NcFileBuilder::new();
        let axes = axes(&mut builder);
        filled_variable(&mut builder, "t", &axes.grid(), &[FILL, FILL, 282.0, 283.0, 284.0, 285.0], FILL, &[]);
        filled_variable(&mut builder, "r", &axes.grid(), &[50.0, FILL, 52.0, 53.0, 54.0, 55.0], FILL, &[]);

        let records = ingested(&builder);
        assert_eq!(records.len(), 5);
        assert!(!records[0].data().contains_key("t"));
        assert_eq!(records[0].data().get("r"), Some(&json!(50.0)));
        // The cell at longitude 14.5 is filled in both variables, so the next record is at 15.0.
        assert_eq!(records[1].data().get("longitude"), Some(&json!(15.0)));
    }

    #[test]
    fn a_two_dimensional_and_a_three_dimensional_variable_both_emit_records() {
        let (mut builder, axes, _) = temperature_and_humidity();
        variable(
            &mut builder,
            "orography",
            &[axes.latitude, axes.longitude],
            &[300.0, 310.0, 320.0, 330.0, 340.0, 350.0],
            &[("units", "m")],
        );

        let records = ingested(&builder);
        assert_eq!(records.len(), 12);
        let surface: Vec<&Record> = records.iter().filter(|record| record.data().contains_key("orography")).collect();
        assert_eq!(surface.len(), 6);
        assert!(
            surface
                .iter()
                .all(|record| !record.data().contains_key("time") && !record.data().contains_key("t"))
        );
    }

    #[test]
    fn a_variable_stored_longitude_first_pairs_each_value_with_its_coordinates() {
        let mut builder = NcFileBuilder::new();
        let axes = axes(&mut builder);
        // Stored (lon, lat): the value at lon 14.5, lat 46.5 is the fourth, 3.0.
        variable(&mut builder, "t", &[axes.longitude, axes.latitude], &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0], &[]);

        let records = ingested(&builder);
        let cell = records
            .iter()
            .find(|record| record.data().get("latitude") == Some(&json!(46.5)) && record.data().get("longitude") == Some(&json!(14.5)))
            .unwrap();
        assert_eq!(cell.data().get("t"), Some(&json!(3.0)));
    }

    #[test]
    fn a_coordinate_of_unknown_role_is_carried_under_its_own_name() {
        let mut builder = NcFileBuilder::new();
        let axes = axes(&mut builder);
        let member = coordinate(&mut builder, "number", &[0.0, 1.0], &[("long_name", "ensemble member numerical id")]);
        variable(
            &mut builder,
            "t",
            &[member, axes.latitude, axes.longitude],
            &[280.0, 281.0, 282.0, 283.0, 284.0, 285.0, 290.0, 291.0, 292.0, 293.0, 294.0, 295.0],
            &[],
        );

        let records = ingested(&builder);
        assert_eq!(records.len(), 12);
        assert_eq!(records[0].data().get("number"), Some(&json!(0)));
        assert_eq!(records[6].data().get("number"), Some(&json!(1)));
    }

    #[test]
    fn a_variable_whose_dimension_has_no_coordinate_is_skipped_and_the_rest_are_ingested() {
        let (mut builder, axes, _) = temperature_and_humidity();
        let bands = builder.add_dimension("band", 2).unwrap();
        variable(
            &mut builder,
            "radiance",
            &[bands, axes.latitude, axes.longitude],
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0],
            &[],
        );

        let records = ingested(&builder);
        assert_eq!(records.len(), 6);
        assert!(records.iter().all(|record| !record.data().contains_key("radiance")));
    }

    #[test]
    fn a_file_holding_only_projected_data_has_no_record_variables() {
        let mut builder = NcFileBuilder::new();
        let y = coordinate(
            &mut builder,
            "y",
            &[0.0, 1000.0],
            &[("standard_name", "projection_y_coordinate"), ("units", "m")],
        );
        let x = coordinate(&mut builder, "x", &[0.0, 1000.0], &[("axis", "X"), ("units", "m")]);
        variable(&mut builder, "t", &[y, x], &[280.0, 281.0, 282.0, 283.0], &[]);

        assert!(matches!(failure(&builder), IngestorError::NetCdf(NetCdfIngestError::NoRecordVariables)));
    }

    #[test]
    fn a_forecast_reference_time_dimension_is_emitted_as_a_timestamp_under_its_own_name() {
        let mut builder = NcFileBuilder::new();
        let reference = coordinate(
            &mut builder,
            "forecast_reference_time",
            &[12.0],
            &[("standard_name", "forecast_reference_time"), ("units", "hours since 2026-01-01")],
        );
        let [latitude, longitude] = single_cell(&mut builder);
        variable(&mut builder, "t", &[reference, latitude, longitude], &[280.0], &[]);

        let record = &ingested(&builder)[0];
        assert_eq!(record.data().get("forecast_reference_time"), Some(&json!("2026-01-01T12:00:00+00:00")));
        assert!(!record.data().contains_key("time"));
    }

    #[test]
    fn a_data_variable_named_like_the_time_key_collides() {
        let mut builder = NcFileBuilder::new();
        let valid = coordinate(&mut builder, "t", &[0.0], &[("standard_name", "time"), ("units", "hours since 2026-01-01")]);
        let [latitude, longitude] = single_cell(&mut builder);
        variable(&mut builder, "time", &[valid, latitude, longitude], &[280.0], &[]);

        let error = failure(&builder);
        assert!(
            matches!(&error, IngestorError::NetCdf(NetCdfIngestError::ColumnCollision { variable }) if variable == "time"),
            "{error:?}"
        );
    }

    #[test]
    fn a_compressed_netcdf4_file_gives_the_same_records_as_the_classic_file() {
        let (classic, _, _) = temperature_and_humidity();
        let (mut deflated, _, variables) = temperature_and_humidity();
        for variable in variables {
            deflated.set_variable_deflate(variable, Some(4), true).unwrap();
        }

        let classic = ingested_as(&classic, NcWriteFormat::Classic);
        let netcdf4 = ingested_as(&deflated, NcWriteFormat::Nc4);
        assert_eq!(classic.len(), 6);
        assert_eq!(data(&netcdf4), data(&classic));
    }

    #[test]
    fn records_are_sent_in_batches_of_the_configured_size() {
        let mut builder = NcFileBuilder::new();
        let latitude = coordinate(&mut builder, "lat", &[46.0, 46.5, 47.0], &[("units", "degrees_north")]);
        let longitude = coordinate(&mut builder, "lon", &[14.0, 14.5], &[("units", "degrees_east")]);
        filled_variable(&mut builder, "t", &[latitude, longitude], &[1.0, 2.0, FILL, 4.0, 5.0, 6.0], FILL, &[]);
        let dir = TempDir::new().unwrap();

        let batches = ingest_batches(&written(&dir, &builder, NcWriteFormat::Classic), 2).unwrap();
        assert_eq!(batches.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 2, 1]);
    }

    #[test]
    fn a_character_data_variable_is_skipped() {
        let (mut builder, axes, _) = temperature_and_humidity();
        let label = builder.add_char_variable("label", &[axes.latitude, axes.longitude]).unwrap();
        builder.write_char_variable(label, b"abcdef").unwrap();

        let records = ingested(&builder);
        assert_eq!(records.len(), 6);
        assert!(records.iter().all(|record| !record.data().contains_key("label")));
    }

    #[test]
    fn a_bytes_payload_is_refused_because_the_reader_needs_a_file() {
        let payload = ProfiledPayload::new(
            CollectedPayload::Bytes(BytesPayload::new(b"CDF\x01".to_vec(), Some(DataFormat::NetCdf), EncodingHint::NONE)),
            Profile::new(DataFormat::NetCdf, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        );
        let error = NetCdfIngestor::from_payload(payload, 8).unwrap_err();
        assert!(matches!(error, IngestorError::NetCdf(NetCdfIngestError::RequiresFile)));
    }

    #[test]
    fn a_file_that_is_not_netcdf_is_a_read_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("table.nc");
        fs::write(&path, "id,name\n1,alpha\n").unwrap();

        let error = NetCdfIngestor::from_payload(payload(&path), 8).unwrap_err();
        assert!(matches!(error, IngestorError::NetCdf(NetCdfIngestError::Read(_))));
    }

    #[test]
    fn a_consumed_ingestor_cannot_be_streamed_again() {
        let ingestor = NetCdfIngestor { file: None, batch_size: 8 };
        let (sender, _receiver) = sync_channel(4);
        let result = Box::new(ingestor).ingest(ChannelSender::bounded(sender));
        assert!(matches!(result, Err(IngestorError::CannotBeStreamedTwice)));
    }
}
