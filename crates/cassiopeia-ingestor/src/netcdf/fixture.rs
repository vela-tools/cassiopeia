use crate::{error::IngestorError, ingestor::Ingestor, netcdf::ingestor::NetCdfIngestor};
use cassiopeia_common::{channel::ChannelSender, format::DataFormat, signal::Signal};
use cassiopeia_data_profiler::profile::Profile;
use cassiopeia_encoding_detection::encoding_hint::EncodingHint;
use cassiopeia_ir::{
    payload::{CollectedPayload, FilePayload, ProfiledPayload},
    record::Record,
};
use mediatype::media_type;
use netcdf_writer::{DimensionId, NcAttrValue, NcFileBuilder, NcWriteFormat, NcWriteOptions, VariableId};
use std::{fs, path::Path, sync::mpsc::sync_channel, thread};

/// A character attribute value.
pub(crate) fn text(value: &str) -> NcAttrValue {
    NcAttrValue::Chars(value.to_string())
}

/// Adds character attributes to a variable.
pub(crate) fn annotate(builder: &mut NcFileBuilder, variable: VariableId, attributes: &[(&str, &str)]) {
    for (name, value) in attributes {
        builder.add_variable_attribute(variable, *name, text(value)).unwrap();
    }
}

/// Adds a dimension with a double coordinate variable of the same name holding `values`.
pub(crate) fn coordinate(builder: &mut NcFileBuilder, name: &str, values: &[f64], attributes: &[(&str, &str)]) -> DimensionId {
    let dimension = builder.add_dimension(name, u64::try_from(values.len()).unwrap()).unwrap();
    variable(builder, name, &[dimension], values, attributes);
    dimension
}

/// Adds a double variable over `dimensions` holding `values`.
pub(crate) fn variable(builder: &mut NcFileBuilder, name: &str, dimensions: &[DimensionId], values: &[f64], attributes: &[(&str, &str)]) -> VariableId {
    let variable = builder.add_variable::<f64>(name, dimensions).unwrap();
    annotate(builder, variable, attributes);
    builder.write_variable(variable, values).unwrap();
    variable
}

/// Adds a double variable over `dimensions` holding `values`, where `fill` marks a missing value.
pub(crate) fn filled_variable(
    builder: &mut NcFileBuilder,
    name: &str,
    dimensions: &[DimensionId],
    values: &[f64],
    fill: f64,
    attributes: &[(&str, &str)],
) -> VariableId {
    let variable = builder.add_variable::<f64>(name, dimensions).unwrap();
    annotate(builder, variable, attributes);
    builder.set_variable_fill_value(variable, fill).unwrap();
    builder.write_variable(variable, values).unwrap();
    variable
}

/// Writes the built file to `path` in `format`.
pub(crate) fn write(builder: &NcFileBuilder, path: &Path, format: NcWriteFormat) {
    let (_, bytes) = builder.to_vec(NcWriteOptions { format }).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Wraps a file on disk as a payload the profiler identified as `NetCDF`.
pub(crate) fn payload(path: &Path) -> ProfiledPayload {
    ProfiledPayload::new(
        CollectedPayload::File(FilePayload::local(path.to_path_buf(), Some(DataFormat::NetCdf), EncodingHint::NONE)),
        Profile::new(DataFormat::NetCdf, media_type!(APPLICATION / OCTET_STREAM), 1.0),
    )
}

/// Ingests the file at `path`, returning each batch the ingestor sent.
pub(crate) fn ingest_batches(path: &Path, batch_size: usize) -> Result<Vec<Vec<Record>>, IngestorError> {
    let ingestor = NetCdfIngestor::from_payload(payload(path), batch_size)?;
    let (sender, receiver) = sync_channel(64);
    let handle = thread::spawn(move || Box::new(ingestor).ingest(ChannelSender::bounded(sender)));
    let batches = receiver
        .iter()
        .map(|signal| match signal {
            Signal::Data(records) => records,
            Signal::Start | Signal::Stop | Signal::Meta(_) | Signal::Error(_) => panic!("expected a data signal"),
        })
        .collect();
    handle.join().unwrap().map(|()| batches)
}

/// Ingests the file at `path`, returning every record in order.
pub(crate) fn ingest(path: &Path) -> Result<Vec<Record>, IngestorError> {
    Ok(ingest_batches(path, 4096)?.into_iter().flatten().collect())
}
