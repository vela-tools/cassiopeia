//! Synthetic GRIB2 files and the drain of a decoded slice set, shared by the GRIB tests.

use crate::grib::grid_slice::GridSliceSet;
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::record::Record;
use grib::{
    WriteToBuffer,
    def::grib2::template::{
        Template3_0,
        param_set::{EarthShape, LatLonGrid as PsLatLon, ScaledValue},
    },
    encoder::{EncodingMethod, GpvEncoder, LatLonGrid, SimplePackingStrategy, SingleGrib2Message, WriteGrib2Message},
};
use std::{borrow::Cow, fs::File, io::Write, path::PathBuf, sync::mpsc::sync_channel, thread};
use temp_dir::TempDir;

/// One gridded field (one GRIB2 submessage) in a synthetic fixture.
pub(crate) struct Field {
    pub(crate) category: u8,
    pub(crate) number: u8,
    pub(crate) ni: usize,
    pub(crate) nj: usize,
    pub(crate) forecast_hours: u32,
    pub(crate) data: Vec<f64>,
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

/// Encodes one GRIB2 message (one submessage) for a field, using simple packing with two decimal
/// digits so the lean decoder can read it back. Masked cells are expressed as NaN in `data`.
fn grib2_message(field: &Field) -> Vec<u8> {
    let values = GpvEncoder::new(Cow::Borrowed(&field.data), EncodingMethod::SimplePacking(SimplePackingStrategy::Decimal(2)));
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

/// Concatenates one message per field into a single GRIB2 stream in `dir` and returns its path.
pub(crate) fn grib2_file(dir: &TempDir, fields: &[Field]) -> PathBuf {
    let mut bytes = Vec::new();
    for field in fields {
        bytes.extend_from_slice(&grib2_message(field));
    }
    let path = dir.path().join("data.grib2");
    File::create(&path).unwrap().write_all(&bytes).unwrap();
    path
}

/// Drains a slice set's records through a buffered channel, for tests that decode into a set
/// directly rather than through the ingestor.
pub(crate) fn drain(set: GridSliceSet) -> Vec<Record> {
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
