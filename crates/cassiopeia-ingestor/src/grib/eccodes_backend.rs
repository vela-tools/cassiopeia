use crate::grib::{
    error::GribIngestError,
    field::{DecodedField, normalize_longitude},
    grid_slice::GridSliceSet,
    level::GribLevel,
    parameter::{ParameterName, parameter_key_grib1, parameter_key_grib2},
};
use cassiopeia_data_profiler::metadata::GribEdition;
use eccodes::{
    FallibleIterator,
    codes_file::{CodesFile, ProductKind},
    codes_message::{CodesMessage, KeyRead},
};
use std::{fmt::Debug, path::Path};

/// ecCodes' default sentinel for a bitmap-masked value; substituted for `NaN` so the pivot omits it.
const MISSING_VALUE: f64 = 9999.0;

/// Decodes a GRIB file with ecCodes, folding each message into `set` as a [`DecodedField`].
///
/// ecCodes decodes every GRIB grid, including the Lambert conformal and polar stereographic grids of
/// regional products, and computes per-cell latitude and longitude for all of them, which the
/// pure-Rust grib-rs path cannot. Each message is one 2D field for a single parameter; fields sharing
/// a grid, level, and time pivot into columns of one location record.
///
/// Only the parameter identity differs by edition: a GRIB2 message carries the
/// `(discipline, parameterCategory, parameterNumber)` triple, a GRIB1 message the
/// `indicatorOfParameter`. Level, values, coordinates, times, and grid identity are read the same way
/// for both, because ecCodes exposes them under one set of keys.
///
/// # Errors
///
/// Returns [`GribIngestError::Eccodes`] if the file cannot be opened as GRIB or a message key cannot be
/// read, or [`GribIngestError::ParameterCode`] if a GRIB2 identity code does not fit its byte range.
pub fn decode(path: &Path, set: &mut GridSliceSet, edition: GribEdition) -> Result<(), GribIngestError> {
    let mut file = CodesFile::new_from_file(path, ProductKind::GRIB)?;
    let mut messages = file.ref_message_iter();
    while let Some(message) = messages.next()? {
        let parameter = read_parameter(&message, edition)?;

        // When a bitmap is present ecCodes fills masked cells with `missingValue`; map that sentinel
        // back to `NaN` so the pivot drops those cells, matching the grib-rs path's NaN semantics.
        let raw: Vec<f64> = message.read_key("values")?;
        let bitmap_present: i64 = message.read_key("bitmapPresent")?;
        let values: Vec<f64> = if bitmap_present == 0 {
            raw
        } else {
            let missing: f64 = message.read_key("missingValue").unwrap_or(MISSING_VALUE);
            raw.into_iter()
                .map(|value| if value.to_bits() == missing.to_bits() { f64::NAN } else { value })
                .collect()
        };

        // ecCodes yields latitudes and longitudes in the same scanning order as `values`. It reports
        // longitude in the grid's own convention, so a `[0, 360)` grid such as the GFS global product
        // needs folding to match the `[-180, 180)` range grib-rs produces and `DecodedField` promises.
        let latitudes: Vec<f64> = message.read_key("latitudes")?;
        let longitudes: Vec<f64> = message.read_key("longitudes")?;
        let latlons: Vec<(f64, f64)> = latitudes.into_iter().zip(longitudes.into_iter().map(normalize_longitude)).collect();

        // `typeOfLevel` (ecCodes uses it for both editions) and `level` are optional: a product without
        // a typed level leaves the facet absent rather than failing the decode. `level` is a native
        // `long`, so it is read as `i64` (ecCodes rejects reading it straight to a double) and widened
        // during normalization.
        let type_of_level: Option<String> = message.read_key("typeOfLevel").ok();
        let level_value: Option<i64> = message.read_key("level").ok();
        let level = type_of_level.map(|kind| GribLevel::from_eccodes(&kind, level_value));

        let reference_time = Some(format_timestamp(message.read_key("dataDate")?, message.read_key("dataTime")?));
        // `validityDate`/`validityTime` are the forecast valid time ecCodes derives from the step.
        let forecast_time = Some(format_timestamp(message.read_key("validityDate")?, message.read_key("validityTime")?));

        // `md5GridSection` hashes the grid definition: byte-equal grids share the hash, so co-located
        // parameters group into one slice without comparing raw section bytes.
        let grid_hash: String = message.read_key("md5GridSection")?;

        set.add(DecodedField {
            grid: grid_hash.into_bytes(),
            parameter,
            latlons,
            values,
            level,
            reference_time,
            forecast_time,
        });
    }

    Ok(())
}

/// Reads a message's canonical parameter key, branching only on the edition's identity source.
///
/// GRIB2 identifies a parameter by its `(discipline, parameterCategory, parameterNumber)` triple: the
/// level-independent identity resolved through the same function grib-rs uses, so both backends agree.
/// GRIB1 identifies it by `indicatorOfParameter`; `table2Version` and `centre` are read only to build
/// the synthetic fallback key for a centre-local parameter ecCodes cannot name.
fn read_parameter<P: Debug>(message: &CodesMessage<P>, edition: GribEdition) -> Result<ParameterName, GribIngestError> {
    match edition {
        GribEdition::V2 => {
            let discipline: i64 = message.read_key("discipline")?;
            let category: i64 = message.read_key("parameterCategory")?;
            let number: i64 = message.read_key("parameterNumber")?;
            Ok(parameter_key_grib2(u8::try_from(discipline)?, u8::try_from(category)?, u8::try_from(number)?))
        }
        GribEdition::V1 => {
            let table: i64 = message.read_key("table2Version")?;
            // `centre` is a code-table key whose native type is the string abbreviation (e.g. "kwbc");
            // the unchecked read takes its numeric code so the synthetic key stays numeric and stable.
            let centre: i64 = message.read_key_unchecked("centre")?;
            let indicator: i64 = message.read_key("indicatorOfParameter")?;
            Ok(parameter_key_grib1(table, centre, indicator))
        }
    }
}

/// Builds an RFC 3339 UTC timestamp from ecCodes' `yyyymmdd` date and `hhmm` time integers.
///
/// ecCodes exposes GRIB dates and times as packed integers (date `20260818`, time `730` for 07:30);
/// this reassembles them without pulling in a date-time dependency, since the components are already
/// validated calendar fields.
fn format_timestamp(date: i64, time: i64) -> String {
    let year = date / 10_000;
    let month = (date / 100) % 100;
    let day = date % 100;
    let hour = time / 100;
    let minute = time % 100;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:00+00:00")
}

#[cfg(test)]
#[cfg(feature = "grib1")]
mod tests {
    use crate::{
        grib::{eccodes_backend, grid_slice::GridSliceSet, ingestor::GribIngestor},
        ingestor::Ingestor,
    };
    use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
    use cassiopeia_data_profiler::{
        metadata::{FormatMetadata, GribEdition, GribMetadata},
        profile::Profile,
    };
    use cassiopeia_ir::{
        payload::{CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use grib_core::{GridDefinition, LatLonGrid, metadata::ReferenceTime};
    use grib_writer::{Grib1Field, Grib1FieldBuilder, Grib1ProductDefinition, GribWriter, PackingStrategy};
    use mediatype::media_type;
    use serde_json::Value;
    use std::{env::var, fs::File, io::Write, path::Path, sync::mpsc::sync_channel, thread};
    use temp_dir::TempDir;

    /// One gridded field in a synthetic GRIB1 fixture: a parameter identity and its per-cell values on
    /// a shared 2x2 lat/lon grid.
    struct Field {
        table_version: u8,
        center_id: u8,
        parameter_number: u8,
        values: Vec<f64>,
    }

    /// A 2x2 regular lat/lon grid: first point (lat 46.2, lon 14.0), last point (lat 46.0, lon 14.3),
    /// scanning west-to-east then north-to-south so cell order is (46.2,14.0), (46.2,14.3), (46.0,14.0),
    /// (46.0,14.3). Coordinates are micro-degrees, as `grib-core` expects.
    fn grid_2x2() -> GridDefinition {
        GridDefinition::LatLon(LatLonGrid {
            ni: 2,
            nj: 2,
            lat_first: 46_200_000,
            lon_first: 14_000_000,
            lat_last: 46_000_000,
            lon_last: 14_300_000,
            di: 300_000,
            dj: 200_000,
            scanning_mode: 0,
        })
    }

    /// Builds the GRIB1 product definition for a field: reference time 2026-08-18T07:30, six-hour
    /// forecast step (valid 13:30), isobaric level 850. A bitmap is declared when any value is NaN.
    fn product(field: &Field) -> Grib1ProductDefinition {
        Grib1ProductDefinition {
            table_version: field.table_version,
            center_id: field.center_id,
            generating_process_id: 255,
            grid_id: 0,
            has_grid_definition: true,
            has_bitmap: field.values.iter().any(|value| value.is_nan()),
            parameter_number: field.parameter_number,
            level_type: 100,
            level_value: 850,
            reference_time: ReferenceTime {
                year: 2026,
                month: 8,
                day: 18,
                hour: 7,
                minute: 30,
                second: 0,
            },
            forecast_time_unit: 1,
            p1: 6,
            p2: 0,
            time_range_indicator: 0,
            average_count: 0,
            missing_count: 0,
            century: 21,
            subcenter_id: 0,
            decimal_scale: 0,
        }
    }

    fn grib1_field(field: &Field) -> Grib1Field {
        Grib1FieldBuilder::new()
            .product(product(field))
            .grid(grid_2x2())
            .packing(PackingStrategy::SimpleAuto { decimal_scale: 0 })
            .values(&field.values)
            .build()
            .unwrap()
    }

    /// Serialises one GRIB1 message per field into a single stream on disk and wraps it as a payload
    /// carrying `edition == V1` metadata, as the profiler would attach.
    fn profiled_grib1(dir: &TempDir, fields: &[Field]) -> ProfiledPayload {
        let mut bytes = Vec::new();
        {
            let mut writer = GribWriter::new(&mut bytes);
            for field in fields {
                writer.write_grib1_message(grib1_field(field)).unwrap();
            }
        }
        let path = dir.path().join("data.grib");
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Grib))),
            Profile::new(DataFormat::Grib, media_type!(APPLICATION / OCTET_STREAM), 1.0)
                .with_metadata(FormatMetadata::Grib(GribMetadata::new(GribEdition::V1))),
        )
    }

    fn temperature(values: Vec<f64>) -> Field {
        Field {
            table_version: 2,
            center_id: 7,
            parameter_number: 11,
            values,
        }
    }

    fn decode_to_set(fields: &[Field]) -> GridSliceSet {
        let dir = TempDir::new().unwrap();
        let mut bytes = Vec::new();
        {
            let mut writer = GribWriter::new(&mut bytes);
            for field in fields {
                writer.write_grib1_message(grib1_field(field)).unwrap();
            }
        }
        let path = dir.path().join("data.grib");
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        let mut set = GridSliceSet::new();
        eccodes_backend::decode(&path, &mut set, GribEdition::V1).unwrap();
        set
    }

    fn ingest_batches(payload: ProfiledPayload, batch_size: usize) -> Vec<Vec<Record>> {
        let ingestor = GribIngestor::from_payload(payload, batch_size).unwrap();
        let (sender, receiver) = sync_channel(64);
        let handle = thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(sender)));
        let batches: Vec<Vec<Record>> = receiver
            .iter()
            .map(|signal| match signal {
                Signal::Data(records) => records,
                Signal::Start | Signal::Stop | Signal::Meta(_) | Signal::Error(_) => panic!("expected a data signal"),
            })
            .collect();
        handle.join().unwrap().unwrap();
        batches
    }

    fn ingest_all(payload: ProfiledPayload) -> Vec<Record> {
        ingest_batches(payload, 4096).into_iter().flatten().collect()
    }

    /// Drains a slice set's records through a buffered channel, for tests that decode into a set
    /// directly rather than through the ingestor.
    fn drain(set: GridSliceSet) -> Vec<Record> {
        // Flush on its own thread: a large set produces more batches than the channel buffers, so the
        // sender would block if this thread were not concurrently draining the receiver.
        let (sender, receiver) = sync_channel(64);
        let handle = thread::spawn(move || set.flush(4096, &ChannelSender::bounded(sender)));
        let records: Vec<Record> = receiver
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
    fn a_single_parameter_grib1_file_becomes_one_record_per_cell() {
        let dir = TempDir::new().unwrap();
        let payload = profiled_grib1(&dir, &[temperature(vec![280.0, 281.0, 282.0, 283.0])]);
        let records = ingest_all(payload);

        assert_eq!(records.len(), 4);
        for record in &records {
            assert_eq!(record.collection(), &None);
            let data = record.data();
            assert!(data.contains_key("latitude"));
            assert!(data.contains_key("longitude"));
            // GRIB1 indicator 11 resolves to the canonical temperature key.
            assert!(data.contains_key("temperature"));
        }
    }

    #[test]
    fn co_located_parameters_pivot_into_columns_of_one_record() {
        let dir = TempDir::new().unwrap();
        let precipitation = Field {
            table_version: 2,
            center_id: 7,
            parameter_number: 61,
            values: vec![0.0, 1.0, 2.0, 3.0],
        };
        let payload = profiled_grib1(&dir, &[temperature(vec![280.0, 281.0, 282.0, 283.0]), precipitation]);
        let records = ingest_all(payload);

        // GRIB1 indicator 11 canonicalizes to `temperature` and indicator 61 to `precip`; both are
        // distinct columns of the same records, which is what the wide-model pivot must produce.
        assert_eq!(records.len(), 4);
        for record in &records {
            let data = record.data();
            assert!(data.contains_key("temperature"));
            assert!(data.contains_key("precip"));
        }
    }

    #[test]
    fn a_bitmap_masks_out_its_cells() {
        let dir = TempDir::new().unwrap();
        let payload = profiled_grib1(&dir, &[temperature(vec![280.0, f64::NAN, 282.0, 283.0])]);
        let records = ingest_all(payload);

        // The single masked cell yields no record; the other three survive.
        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|record| record.data().contains_key("temperature")));
    }

    #[test]
    fn latitude_and_longitude_follow_the_scanning_order() {
        let dir = TempDir::new().unwrap();
        let payload = profiled_grib1(&dir, &[temperature(vec![280.0, 281.0, 282.0, 283.0])]);
        let records = ingest_all(payload);

        let first = records
            .iter()
            .find(|record| {
                record
                    .data()
                    .get("temperature")
                    .and_then(Value::as_f64)
                    .is_some_and(|value| (value - 280.0).abs() < 0.5)
            })
            .unwrap();
        let latitude = first.data().get("latitude").and_then(Value::as_f64).unwrap();
        let longitude = first.data().get("longitude").and_then(Value::as_f64).unwrap();
        assert!((latitude - 46.2).abs() < 1e-2, "latitude was {latitude}");
        assert!((longitude - 14.0).abs() < 1e-2, "longitude was {longitude}");
    }

    #[test]
    fn an_unregistered_local_parameter_becomes_a_synthetic_column() {
        let dir = TempDir::new().unwrap();
        // Indicator 200 is outside the curated Table 2 range, so it falls back to its synthetic
        // identity key rather than a canonical name.
        let local = Field {
            table_version: 2,
            center_id: 0,
            parameter_number: 200,
            values: vec![1.0, 2.0, 3.0, 4.0],
        };
        let payload = profiled_grib1(&dir, &[local]);
        let records = ingest_all(payload);

        assert!(!records.is_empty());
        for record in &records {
            assert!(record.data().contains_key("g1t2c0p200"));
        }
    }

    #[test]
    fn records_are_flushed_in_batches_of_the_configured_size() {
        let dir = TempDir::new().unwrap();
        // Four cells with one masked yields three records; batched by two that is 2, 1.
        let payload = profiled_grib1(&dir, &[temperature(vec![280.0, f64::NAN, 282.0, 283.0])]);
        let batches = ingest_batches(payload, 2);
        let sizes: Vec<usize> = batches.iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![2, 1]);
    }

    #[test]
    fn the_grid_slice_set_groups_two_parameters_into_one_slice() {
        let precipitation = Field {
            table_version: 2,
            center_id: 7,
            parameter_number: 61,
            values: vec![0.0, 1.0, 2.0, 3.0],
        };
        let set = decode_to_set(&[temperature(vec![280.0, 281.0, 282.0, 283.0]), precipitation]);
        let records = drain(set);
        assert_eq!(records.len(), 4);
    }

    #[test]
    fn an_isobaric_level_normalizes_to_its_hectopascal_value() {
        let set = decode_to_set(&[temperature(vec![280.0, 281.0, 282.0, 283.0])]);
        let records = drain(set);
        // The fixture declares level type 100 (isobaric) at 850; ecCodes reports it under
        // `isobaricInhPa`, so the normalized level is the bare token and the hPa number.
        for record in &records {
            assert_eq!(record.data().get("level_type").and_then(Value::as_str), Some("isobaric"));
            assert_eq!(record.data().get("level").and_then(Value::as_f64), Some(850.0));
        }
    }

    #[test]
    fn a_payload_without_metadata_falls_back_to_the_header_edition() {
        let dir = TempDir::new().unwrap();
        // No format metadata: the ingestor must read the edition octet from the file header itself.
        let mut bytes = Vec::new();
        {
            let mut writer = GribWriter::new(&mut bytes);
            writer.write_grib1_message(grib1_field(&temperature(vec![280.0, 281.0, 282.0, 283.0]))).unwrap();
        }
        let path = dir.path().join("data.grib");
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Grib))),
            Profile::new(DataFormat::Grib, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        );

        let records = ingest_all(payload);
        assert_eq!(records.len(), 4);
    }

    #[test]
    #[ignore = "requires an INCA GRIB1 file named by CASSIOPEIA_INCA_GRIB"]
    fn the_inca_lambert_file_decodes_with_coordinates() {
        // ARSO publishes INCA nowcast GRIB1 under
        // https://meteo.arso.gov.si/uploads/probase/www/nowcast/data/. Download one and name it in
        // CASSIOPEIA_INCA_GRIB to exercise the Lambert conformal grid, which the synthetic fixtures
        // above cannot express.
        let Ok(path) = var("CASSIOPEIA_INCA_GRIB") else {
            return;
        };
        let path = Path::new(&path);
        if !path.exists() {
            return;
        }
        let mut set = GridSliceSet::new();
        eccodes_backend::decode(path, &mut set, GribEdition::V1).unwrap();
        let records = drain(set);
        assert!(!records.is_empty());
        assert!(
            records
                .iter()
                .all(|record| record.data().contains_key("latitude") && record.data().contains_key("longitude"))
        );
    }
}
