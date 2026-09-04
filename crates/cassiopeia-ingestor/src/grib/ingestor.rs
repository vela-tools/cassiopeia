#[cfg(any(feature = "grib1", feature = "grib2-full"))]
use crate::grib::eccodes_backend;
#[cfg(not(feature = "grib2-full"))]
use crate::grib::grib_rs_backend;
use crate::{
    error::IngestorError,
    grib::{error::GribIngestError, grid_slice::GridSliceSet},
    ingestor::Ingestor,
};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_data_profiler::metadata::{FormatMetadata, GribEdition, GribMetadata};
use cassiopeia_ir::{
    payload::{CollectedPayload, ProfiledPayload},
    record::Record,
};
#[cfg(not(feature = "grib2-full"))]
use std::io::BufReader;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

/// Ingestor for GRIB gridded binary data, both editions.
///
/// A GRIB file is a sequence of fields, each a 2D grid for one parameter at one vertical level and
/// forecast time. Parameters sharing a grid, level, and time are pivoted into columns of one location
/// record: every non-masked grid cell becomes a record carrying its `(latitude, longitude)` plus one
/// column per co-located parameter, so a mapping can build a single multi-attribute observation entity
/// per location.
///
/// Two compile-time features select the backend per edition, both linking ecCodes and toggling
/// independently. GRIB2 goes through ecCodes with `grib2-full` (the default, covering projected grids
/// and the full parameter tables) and through the pure-Rust grib-rs fallback without it. GRIB1 goes through
/// ecCodes with `grib1` (the default) and is rejected without it. Either backend emits the same
/// Cassiopeia-canonical parameter keys and normalized level, so one mapping works regardless of which
/// decoded the file.
#[derive(Debug)]
pub struct GribIngestor {
    source: Option<PathBuf>,
    edition: GribEdition,
    batch_size: usize,
}

impl GribIngestor {
    /// Creates a GRIB ingestor from a profiled payload.
    ///
    /// The edition is taken from the profiler's inspected [`FormatMetadata::Grib`] metadata when present;
    /// otherwise it is read from the file's Indicator Section, which is authoritative. Resolving it
    /// here keeps an unsupported edition a typed error and picks the decoder before any data is read.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError`] when the payload is bytes rather than a file, the file cannot be
    /// opened or read, or it declares an edition other than 1 or 2.
    pub fn from_payload(payload: ProfiledPayload, batch_size: usize) -> Result<GribIngestor, IngestorError> {
        let inspected = payload
            .profile()
            .metadata()
            .as_ref()
            .and_then(FormatMetadata::as_grib)
            .map(GribMetadata::edition);

        let path = match payload.into_payload() {
            CollectedPayload::File(f) => f.into_path(),
            CollectedPayload::Bytes(_) => return Err(GribIngestError::RequiresFile.into()),
        };

        let edition = match inspected {
            Some(edition) => edition,
            None => read_edition_from_header(&path)?,
        };

        Ok(GribIngestor {
            source: Some(path),
            edition,
            batch_size,
        })
    }
}

impl Ingestor for GribIngestor {
    fn ingest(mut self: Box<Self>, sender: ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let path = self.source.take().ok_or(IngestorError::CannotBeStreamedTwice)?;
        let mut set = GridSliceSet::new();
        match self.edition {
            GribEdition::V2 => decode_edition2(&path, &mut set)?,
            GribEdition::V1 => decode_edition1(&path, &mut set)?,
        }
        set.flush(self.batch_size, &sender)
    }
}

/// Reads the GRIB edition from octet 8 of the Indicator Section when the profiler attached no metadata.
///
/// Both editions place the edition number at the same offset (index 7), so a single read suffices; the
/// header is the authoritative declaration of the format.
fn read_edition_from_header(path: &Path) -> Result<GribEdition, IngestorError> {
    let mut file = File::open(path).map_err(|source| IngestorError::Io {
        source,
        path: path.to_path_buf(),
    })?;
    let mut indicator = [0u8; 8];
    file.read_exact(&mut indicator).map_err(|source| IngestorError::Io {
        source,
        path: path.to_path_buf(),
    })?;
    match indicator[7] {
        1 => Ok(GribEdition::V1),
        2 => Ok(GribEdition::V2),
        other => Err(GribIngestError::UnsupportedEdition(other).into()),
    }
}

/// Decodes GRIB2 with ecCodes when `grib2-full` is enabled: it computes coordinates for projected
/// grids and reads the full parameter tables the pure-Rust fallback cannot.
#[cfg(feature = "grib2-full")]
fn decode_edition2(path: &Path, set: &mut GridSliceSet) -> Result<(), IngestorError> {
    eccodes_backend::decode(path, set, GribEdition::V2).map_err(IngestorError::from)
}

/// Decodes GRIB2 with the pure-Rust grib-rs fallback when `grib2-full` is disabled: no C dependency,
/// but limited to regular latitude/longitude grids.
#[cfg(not(feature = "grib2-full"))]
fn decode_edition2(path: &Path, set: &mut GridSliceSet) -> Result<(), IngestorError> {
    let file = File::open(path).map_err(|source| IngestorError::Io {
        source,
        path: path.to_path_buf(),
    })?;
    grib_rs_backend::decode(BufReader::new(file), set).map_err(IngestorError::from)
}

/// Decodes GRIB1 with ecCodes when the `grib1` feature is enabled.
#[cfg(feature = "grib1")]
fn decode_edition1(path: &Path, set: &mut GridSliceSet) -> Result<(), IngestorError> {
    eccodes_backend::decode(path, set, GribEdition::V1).map_err(IngestorError::from)
}

/// Rejects GRIB1 when the `grib1` feature is disabled: this build links no GRIB1 decoder.
#[cfg(not(feature = "grib1"))]
fn decode_edition1(_path: &Path, _set: &mut GridSliceSet) -> Result<(), IngestorError> {
    Err(GribIngestError::UnsupportedEdition(1).into())
}

#[cfg(test)]
mod tests {
    use crate::{
        error::IngestorError,
        grib::{error::GribIngestError, ingestor::GribIngestor},
        ingestor::Ingestor,
    };
    use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
    #[cfg(not(feature = "grib1"))]
    use cassiopeia_data_profiler::metadata::{FormatMetadata, GribMetadata};
    use cassiopeia_data_profiler::{metadata::GribEdition, profile::Profile};
    use cassiopeia_ir::{
        payload::{BytesPayload, CollectedPayload, FilePayload, ProfiledPayload},
        record::Record,
    };
    use grib::{
        WriteToBuffer,
        def::grib2::template::{
            Template3_0,
            param_set::{EarthShape, LatLonGrid as PsLatLon, ScaledValue},
        },
        encoder::{EncodingMethod, GpvEncoder, LatLonGrid, SimplePackingStrategy, SingleGrib2Message, WriteGrib2Message},
    };
    use mediatype::media_type;
    use serde_json::Value;
    use std::{borrow::Cow, collections::HashMap, fs::File, io::Write, sync::mpsc::sync_channel, thread};
    use temp_dir::TempDir;

    /// One gridded field (one GRIB2 submessage) in a synthetic fixture.
    struct Field {
        category: u8,
        number: u8,
        ni: usize,
        nj: usize,
        forecast_hours: u32,
        data: Vec<f64>,
    }

    /// Serialises Section 1 (Identification) body: reference time 2026-08-18T07:30:00Z.
    fn section1_body() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&0u16.to_be_bytes()); // originating centre
        body.extend_from_slice(&0u16.to_be_bytes()); // originating sub-centre
        body.push(2); // GRIB master tables version
        body.push(0); // GRIB local tables version
        body.push(0); // significance of reference time
        body.extend_from_slice(&2026u16.to_be_bytes()); // year
        body.extend_from_slice(&[8, 18, 7, 30, 0]); // month, day, hour, minute, second
        body.push(0); // production status
        body.push(1); // type of processed data
        body
    }

    /// Serialises a Section 3 (Grid Definition) body for a regular lat/lon grid (template 3.0).
    fn section3_body(ni: usize, nj: usize) -> Vec<u8> {
        let lat_lon: PsLatLon = (&LatLonGrid {
            shape: (ni, nj),
            first_point: (46.2, 14.0),
            last_point: (46.0, 14.3),
            i_consecutive: true,
        })
            .into();
        let template = Template3_0 {
            earth: EarthShape {
                shape: 6,
                spherical_earth_radius: ScaledValue {
                    scale_factor: 0,
                    scaled_value: 6_371_229,
                },
                major_axis: ScaledValue {
                    scale_factor: 0,
                    scaled_value: 0,
                },
                minor_axis: ScaledValue {
                    scale_factor: 0,
                    scaled_value: 0,
                },
            },
            lat_lon,
        };
        let mut template_bytes = vec![0u8; 256];
        let written = template.write_to_buffer(&mut template_bytes).unwrap();
        template_bytes.truncate(written);

        let mut body = Vec::new();
        body.push(0); // source of grid definition
        body.extend_from_slice(&u32::try_from(ni * nj).unwrap().to_be_bytes()); // number of data points
        body.push(0); // number of octets for optional list of numbers
        body.push(0); // interpretation of list of numbers
        body.extend_from_slice(&0u16.to_be_bytes()); // grid definition template number (0 = lat/lon)
        body.extend_from_slice(&template_bytes);
        body
    }

    /// Serialises a Section 4 (Product Definition) body for template 4.0: a forecast at height 2 m,
    /// `forecast_hours` after the reference time.
    fn section4_body(category: u8, number: u8, forecast_hours: u32) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&0u16.to_be_bytes()); // number of coordinate values
        body.extend_from_slice(&0u16.to_be_bytes()); // product definition template number (4.0)
        body.push(category);
        body.push(number);
        body.push(2); // type of generating process
        body.push(0); // background generating process identifier
        body.push(0); // analysis or forecast generating process identifier
        body.extend_from_slice(&0u16.to_be_bytes()); // hours of observational data cutoff
        body.push(0); // minutes of observational data cutoff
        body.push(1); // indicator of unit of time range (1 = hour)
        body.extend_from_slice(&forecast_hours.to_be_bytes()); // forecast time
        body.push(103); // type of first fixed surface (103 = specified height above ground)
        body.push(0); // scale factor of first fixed surface
        body.extend_from_slice(&2u32.to_be_bytes()); // scaled value of first fixed surface (2 m)
        body.push(255); // type of second fixed surface (missing)
        body.push(0xff); // scale factor of second fixed surface (missing)
        body.extend_from_slice(&0xffff_ffffu32.to_be_bytes()); // scaled value of second fixed surface
        body
    }

    /// Encodes one GRIB2 message (one submessage) for a field, using simple packing so the lean
    /// decoder can read it back. Masked cells are expressed as NaN in `data`.
    fn grib2_message(field: &Field) -> Vec<u8> {
        let values = GpvEncoder::new(Cow::Owned(field.data.clone()), EncodingMethod::SimplePacking(SimplePackingStrategy::Decimal(2)));
        // grib 0.18.1 narrowed the section write-trait impls from `AsRef<[u8]>` to `[u8]`, so the
        // section bodies must be passed as slices (bound to locals to outlive the message).
        let ident = section1_body();
        let grid = section3_body(field.ni, field.nj);
        let product = section4_body(field.category, field.number, field.forecast_hours);
        let message = SingleGrib2Message::new(0u8, ident.as_slice(), None::<&[u8]>, grid.as_slice(), product.as_slice(), values);
        let mut buffer = vec![0u8; message.num_octets()];
        let written = message.write(&mut buffer).unwrap();
        buffer.truncate(written);
        buffer
    }

    /// Concatenates one message per field into a single GRIB2 stream on disk and wraps it as a payload.
    fn profiled_grib(dir: &TempDir, fields: &[Field]) -> ProfiledPayload {
        let mut bytes = Vec::new();
        for field in fields {
            bytes.extend_from_slice(&grib2_message(field));
        }
        let path = dir.path().join("data.grib2");
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Grib))),
            Profile::new(DataFormat::Grib, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        )
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

    fn field(category: u8, number: u8, data: Vec<f64>) -> Field {
        Field {
            category,
            number,
            ni: 2,
            nj: 2,
            forecast_hours: 6,
            data,
        }
    }

    #[test]
    fn co_located_parameters_pivot_into_columns_of_one_location_record() {
        let dir = TempDir::new().unwrap();
        let payload = profiled_grib(&dir, &[field(0, 0, vec![280.0, 281.0, 282.0, 283.0]), field(1, 8, vec![0.1, 0.2, 0.3, 0.4])]);
        let records = ingest_all(payload);

        // Two parameters on one 2x2 grid become four location records, each carrying both columns.
        assert_eq!(records.len(), 4);
        for record in &records {
            assert_eq!(record.collection(), &None);
            let data = record.data();
            assert!(data.contains_key("latitude"));
            assert!(data.contains_key("longitude"));
            // Canonical keys from the GRIB2 triple: (0,0,0) -> temperature, (0,1,8) -> precip.
            assert!(data.contains_key("temperature"));
            assert!(data.contains_key("precip"));
            // Section 4 sets surface type 103 at 2 m, normalizing to the height-above-ground level.
            assert_eq!(data.get("level_type").and_then(Value::as_str), Some("height_above_ground"));
            assert_eq!(data.get("level").and_then(Value::as_f64), Some(2.0));
            assert_eq!(data.get("forecastTime"), Some(&Value::String("2026-08-18T13:30:00+00:00".to_string())));
        }
    }

    #[test]
    fn the_same_parameter_at_different_forecast_times_yields_separate_records() {
        let dir = TempDir::new().unwrap();
        let payload = profiled_grib(
            &dir,
            &[
                Field {
                    category: 0,
                    number: 0,
                    ni: 2,
                    nj: 2,
                    forecast_hours: 6,
                    data: vec![280.0, 281.0, 282.0, 283.0],
                },
                Field {
                    category: 0,
                    number: 0,
                    ni: 2,
                    nj: 2,
                    forecast_hours: 12,
                    data: vec![284.0, 285.0, 286.0, 287.0],
                },
            ],
        );
        let records = ingest_all(payload);

        // Two forecast steps are two slices: four cells each, eight records, split by valid time.
        assert_eq!(records.len(), 8);
        let mut by_time: HashMap<String, usize> = HashMap::new();
        for record in &records {
            let time = record.data().get("forecastTime").and_then(Value::as_str).unwrap().to_string();
            *by_time.entry(time).or_default() += 1;
        }
        assert_eq!(by_time.get("2026-08-18T13:30:00+00:00"), Some(&4));
        assert_eq!(by_time.get("2026-08-18T19:30:00+00:00"), Some(&4));
    }

    #[test]
    fn a_masked_parameter_is_dropped_from_the_cell_but_co_located_values_remain() {
        let dir = TempDir::new().unwrap();
        let payload = profiled_grib(&dir, &[field(0, 0, vec![280.0, f64::NAN, 282.0, 283.0]), field(1, 8, vec![0.1, 0.2, 0.3, 0.4])]);
        let records = ingest_all(payload);

        // Every cell still has precipitation, so all four records survive; one lacks temperature.
        assert_eq!(records.len(), 4);
        let without_temperature = records.iter().filter(|record| !record.data().contains_key("temperature")).count();
        assert_eq!(without_temperature, 1);
        assert!(records.iter().all(|record| record.data().contains_key("precip")));
    }

    #[test]
    fn a_cell_masked_in_every_parameter_produces_no_record() {
        let dir = TempDir::new().unwrap();
        let payload = profiled_grib(&dir, &[field(0, 0, vec![280.0, f64::NAN, 282.0, 283.0])]);
        let records = ingest_all(payload);
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn longitude_and_latitude_are_emitted_in_the_correct_order() {
        let dir = TempDir::new().unwrap();
        // First grid point is at (lat 46.2, lon 14.0); a lat/lon swap would be caught here.
        let payload = profiled_grib(&dir, &[field(0, 0, vec![280.0, 281.0, 282.0, 283.0])]);
        let records = ingest_all(payload);

        let first = records
            .iter()
            .find(|record| (record.data().get("temperature").and_then(Value::as_f64).unwrap() - 280.0).abs() < 0.5)
            .unwrap();
        let latitude = first.data().get("latitude").and_then(Value::as_f64).unwrap();
        let longitude = first.data().get("longitude").and_then(Value::as_f64).unwrap();
        assert!((latitude - 46.2).abs() < 1e-2, "latitude was {latitude}");
        assert!((longitude - 14.0).abs() < 1e-2, "longitude was {longitude}");
    }

    #[test]
    fn an_unknown_parameter_becomes_a_synthetic_column() {
        let dir = TempDir::new().unwrap();
        let payload = profiled_grib(&dir, &[field(99, 5, vec![1.0, 2.0, 3.0, 4.0])]);
        let records = ingest_all(payload);
        assert!(!records.is_empty());
        for record in &records {
            assert!(record.data().contains_key("d0c99n5"));
        }
    }

    #[test]
    fn a_grib2_payload_routes_through_the_header_when_metadata_is_absent() {
        let dir = TempDir::new().unwrap();
        // profiled_grib attaches no edition metadata, so the header read must resolve edition 2.
        let payload = profiled_grib(&dir, &[field(0, 0, vec![280.0, 281.0, 282.0, 283.0])]);
        let records = ingest_all(payload);
        assert_eq!(records.len(), 4);
    }

    #[cfg(not(feature = "grib1"))]
    #[test]
    fn a_grib1_file_is_rejected_when_the_grib1_feature_is_off() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("edition1.grib");
        // GRIB Indicator Section with edition octet 1; without the grib1 feature this build has no
        // decoder, so the rejection surfaces at ingest time as a typed edition error.
        File::create(&path).unwrap().write_all(b"GRIB\x00\x00\x1c\x01\x00\x00\x00\x00").unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Grib))),
            Profile::new(DataFormat::Grib, media_type!(APPLICATION / OCTET_STREAM), 1.0)
                .with_metadata(FormatMetadata::Grib(GribMetadata::new(GribEdition::V1))),
        );

        let ingestor = GribIngestor::from_payload(payload, 8).unwrap();
        let (sender, _receiver) = sync_channel(4);
        let error = Box::new(ingestor).ingest(ChannelSender::bounded(sender)).unwrap_err();
        assert!(matches!(error, IngestorError::Grib(GribIngestError::UnsupportedEdition(1))));
    }

    #[test]
    fn an_unsupported_edition_is_rejected_from_the_header() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("edition9.grib");
        // Edition octet 9: no decoder exists for it in either build.
        File::create(&path).unwrap().write_all(b"GRIB\x00\x00\x1c\x09\x00\x00\x00\x00").unwrap();
        let payload = ProfiledPayload::new(
            CollectedPayload::File(FilePayload::new(path, Some(DataFormat::Grib))),
            Profile::new(DataFormat::Grib, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        );

        let error = GribIngestor::from_payload(payload, 8).unwrap_err();
        assert!(matches!(error, IngestorError::Grib(GribIngestError::UnsupportedEdition(9))));
    }

    #[test]
    fn a_bytes_payload_is_rejected_because_grib_needs_a_file() {
        let payload = ProfiledPayload::new(
            CollectedPayload::Bytes(BytesPayload::new(b"GRIB\x00\x00\x00\x02".to_vec(), Some(DataFormat::Grib))),
            Profile::new(DataFormat::Grib, media_type!(APPLICATION / OCTET_STREAM), 1.0),
        );
        let error = GribIngestor::from_payload(payload, 8).unwrap_err();
        assert!(matches!(error, IngestorError::Grib(GribIngestError::RequiresFile)));
    }

    #[test]
    fn a_consumed_ingestor_cannot_be_streamed_again() {
        let ingestor = GribIngestor {
            source: None,
            edition: GribEdition::V2,
            batch_size: 8,
        };
        let (sender, _receiver) = sync_channel(4);
        let result = Box::new(ingestor).ingest(ChannelSender::bounded(sender));
        assert!(matches!(result, Err(IngestorError::CannotBeStreamedTwice)));
    }

    #[test]
    fn records_are_flushed_in_batches_of_the_configured_size() {
        let dir = TempDir::new().unwrap();
        // A 3x2 grid with one masked cell yields five records; batched by two that is 2, 2, 1.
        let payload = profiled_grib(
            &dir,
            &[Field {
                category: 0,
                number: 0,
                ni: 3,
                nj: 2,
                forecast_hours: 6,
                data: vec![280.0, 281.0, 282.0, f64::NAN, 284.0, 285.0],
            }],
        );
        let batches = ingest_batches(payload, 2);
        let sizes: Vec<usize> = batches.iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![2, 2, 1]);
    }
}
