//! End-to-end check of the `NetCDF` handoff from the profiler to the ingestor.
//!
//! The fixtures are written by `netcdf-writer` in the classic and the `NetCDF-4` format, profiled from
//! their path exactly as the pipeline would ([`cassiopeia_data_profiler::profile_path`]), then the
//! resulting profile rides into [`NetCdfIngestor`].
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration-test fixtures unwrap on setup failure and panic on an unexpected signal, which is the intended way for the test to abort"
)]

use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
use cassiopeia_data_profiler::profile_path;
use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
use cassiopeia_ingestor::{ingestor::Ingestor, netcdf::ingestor::NetCdfIngestor};
use cassiopeia_ir::{
    payload::{CollectedPayload, FilePayload, ProfiledPayload},
    record::Record,
};
use netcdf_writer::{DimensionId, NcAttrValue, NcFileBuilder, NcWriteFormat, NcWriteOptions};
use pretty_assertions::assert_eq;
use std::{fs, path::Path, sync::mpsc::sync_channel, thread};
use temp_dir::TempDir;

/// Adds a dimension with a double coordinate variable of the same name and one character attribute.
fn coordinate(builder: &mut NcFileBuilder, name: &str, values: &[f64], units: &str) -> DimensionId {
    let dimension = builder.add_dimension(name, u64::try_from(values.len()).unwrap()).unwrap();
    let variable = builder.add_variable::<f64>(name, &[dimension]).unwrap();
    builder
        .add_variable_attribute(variable, "units", NcAttrValue::Chars(units.to_string()))
        .unwrap();
    builder.write_variable(variable, values).unwrap();
    dimension
}

/// Writes a two-time, 2 x 3 temperature grid, deflated when written as `NetCDF-4`.
fn write_grid(path: &Path, format: NcWriteFormat) {
    let mut builder = NcFileBuilder::new();
    let time = coordinate(&mut builder, "time", &[0.0, 6.0], "hours since 2026-01-01 00:00:00");
    let latitude = coordinate(&mut builder, "lat", &[46.0, 46.5], "degrees_north");
    let longitude = coordinate(&mut builder, "lon", &[14.0, 14.5, 15.0], "degrees_east");
    let temperature = builder.add_variable::<f32>("t", &[time, latitude, longitude]).unwrap();
    builder
        .write_variable(
            temperature,
            &[280.0_f32, 281.0, 282.0, 283.0, 284.0, 285.0, 286.0, 287.0, 288.0, 289.0, 290.0, 291.0],
        )
        .unwrap();
    if format == NcWriteFormat::Nc4 {
        builder.set_variable_deflate(temperature, Some(4), true).unwrap();
    }
    let (_, bytes) = builder.to_vec(NcWriteOptions { format }).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Profiles the path exactly as the pipeline would, asserts it is `NetCDF`, and pairs it with a
/// payload.
fn profiled(path: &Path) -> ProfiledPayload {
    let profile = profile_path(path, &EncodingHint::NONE).unwrap();
    assert_eq!(*profile.format(), DataFormat::NetCdf);
    assert_eq!(profile.mime_type().to_string(), "application/x-netcdf");
    ProfiledPayload::new(
        CollectedPayload::File(FilePayload::local(path.to_path_buf(), Some(DataFormat::NetCdf), EncodingHint::NONE)),
        profile,
    )
}

fn ingest(payload: ProfiledPayload) -> Vec<Record> {
    let ingestor = NetCdfIngestor::from_payload(payload, 8).unwrap();
    let (tx, rx) = sync_channel(8);
    let handle = thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(tx)));
    let records: Vec<Record> = rx
        .iter()
        .flat_map(|signal| match signal {
            Signal::Data(records) => records,
            Signal::Start | Signal::Stop | Signal::Meta(_) | Signal::Error(_) => panic!("expected a data signal"),
        })
        .collect();
    handle.join().unwrap().unwrap();
    records
}

#[test]
fn a_classic_grid_profiles_as_netcdf_and_ingests_into_one_record_per_cell_and_time() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("grid.nc");
    write_grid(&path, NcWriteFormat::Classic);

    let records = ingest(profiled(&path));
    assert_eq!(records.len(), 12);
    assert!(records.iter().all(|record| record.collection().is_none() && record.data().contains_key("t")));
}

#[test]
fn a_compressed_netcdf4_grid_profiles_as_netcdf_and_ingests_into_one_record_per_cell_and_time() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("grid.nc");
    write_grid(&path, NcWriteFormat::Nc4);

    let records = ingest(profiled(&path));
    assert_eq!(records.len(), 12);
    assert_eq!(
        records.last().and_then(|record| record.data().get("time")).and_then(|time| time.as_str()),
        Some("2026-01-01T06:00:00+00:00")
    );
}
