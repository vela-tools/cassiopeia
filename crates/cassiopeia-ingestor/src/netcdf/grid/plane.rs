use crate::{
    error::IngestorError,
    location_record::{facets::LocationFacets, longitude::normalize_longitude, parameter_name::ParameterName, record::location_record},
    netcdf::{
        coordinate_facets::CoordinateFacets,
        decode::Decode,
        dimension_length::{dimension_length, too_long},
        error::NetCdfIngestError,
        grid::{
            auxiliary::AuxiliaryAxis,
            plane_indices::PlaneIndices,
            variables::{GridGroup, GridLayout},
        },
        nonexistent_dates::NonexistentDateLog,
    },
    record_batches::RecordBatches,
};
use netcdf_reader::{NcFile, NcSliceInfo, NcSliceInfoElem};

/// A grid group's coordinates, read once and decoded into what its records carry.
struct GridCoordinates<'k> {
    /// The latitude of each index along the latitude dimension.
    latitudes: Vec<f64>,
    /// The longitude of each index along the longitude dimension, folded into `[-180, 180)`.
    longitudes: Vec<f64>,
    /// Each facet axis's decoded values, in facet order.
    facets: Vec<CoordinateFacets<'k>>,
    /// Each auxiliary axis with its decoded values.
    auxiliaries: Vec<(&'k AuxiliaryAxis<'k>, CoordinateFacets<'k>)>,
}

impl<'k> GridCoordinates<'k> {
    /// Reads every coordinate of `layout`, recording in `nonexistent` the dates of its time coordinates
    /// the Gregorian calendar does not have. Dimension coordinates may not hold missing values (CF 1.11
    /// §5), so they are only unpacked; auxiliary coordinates may, so theirs are masked too.
    fn read(file: &NcFile, layout: &'k GridLayout<'k>, nonexistent: &mut NonexistentDateLog) -> Result<GridCoordinates<'k>, NetCdfIngestError> {
        let latitudes = Decode::Unpack.read(file, layout.latitude.coordinate)?;
        let longitudes = Decode::Unpack
            .read(file, layout.longitude.coordinate)?
            .into_iter()
            .map(normalize_longitude)
            .collect();
        let facets = layout
            .facets
            .iter()
            .map(|axis| CoordinateFacets::read(file, axis.coordinate, &axis.key, Decode::Unpack, nonexistent))
            .collect::<Result<_, _>>()?;
        let auxiliaries = layout
            .auxiliaries
            .iter()
            .map(|axis| {
                Ok((
                    axis,
                    CoordinateFacets::read(file, axis.coordinate, &axis.key, Decode::MaskAndUnpack, nonexistent)?,
                ))
            })
            .collect::<Result<_, NetCdfIngestError>>()?;
        Ok(GridCoordinates {
            latitudes,
            longitudes,
            facets,
            auxiliaries,
        })
    }

    /// The facets every record of the plane at `indices` shares: its facet axes' values and those of
    /// the auxiliary axes.
    fn plane_facets(&self, indices: &[usize]) -> LocationFacets {
        let facets = self
            .facets
            .iter()
            .zip(indices)
            .fold(LocationFacets::default(), |facets, (coordinate, &index)| coordinate.stamp(index, facets));
        self.auxiliaries.iter().fold(facets, |facets, (axis, coordinate)| match axis.index(indices) {
            Some(index) => coordinate.stamp(index, facets),
            None => facets,
        })
    }
}

/// Emits one location record per cell of every latitude-longitude plane of a grid group.
///
/// The group's coordinates are read once: longitudes are folded into `[-180, 180)`, and every other
/// coordinate is decoded into the facets its plane's records share. The planes are the combinations of
/// the non-horizontal dimensions; for each, every variable's plane is read with its fill values masked
/// and its packing undone, so memory holds one latitude-longitude plane per variable, never a whole
/// variable. Cells are indexed by the positions of the latitude and longitude dimensions, so any
/// dimension order pairs values with the right coordinates. The dates of its time coordinates that the
/// Gregorian calendar does not have are recorded in `nonexistent`.
///
/// # Errors
///
/// Returns [`IngestorError::NetCdf`] when a coordinate or plane cannot be read or decoded, and
/// [`IngestorError::ChannelClosed`] when the downstream channel has closed.
pub fn ingest_grid_group(
    file: &NcFile,
    group: &GridGroup<'_>,
    nonexistent: &mut NonexistentDateLog,
    batches: &mut RecordBatches<'_>,
) -> Result<(), IngestorError> {
    let layout = &group.layout;
    let coordinates = GridCoordinates::read(file, layout, nonexistent)?;
    let sizes = layout
        .facets
        .iter()
        .filter_map(|axis| layout.dimensions.get(axis.position))
        .map(dimension_length)
        .collect::<Result<Vec<_>, _>>()?;
    let columns: Vec<ParameterName> = group.variables.iter().map(|variable| ParameterName::from(variable.name())).collect();

    for indices in PlaneIndices::new(sizes) {
        let plane_facets = coordinates.plane_facets(&indices);
        let selection = plane_selection(layout, &indices)?;
        let planes = group
            .variables
            .iter()
            .map(|variable| Decode::MaskAndUnpack.read_slice(file, variable, &selection))
            .collect::<Result<Vec<_>, _>>()
            .map_err(NetCdfIngestError::from)?;

        for (latitude_index, &latitude) in coordinates.latitudes.iter().enumerate() {
            for (longitude_index, &longitude) in coordinates.longitudes.iter().enumerate() {
                let cell = if layout.latitude.position < layout.longitude.position {
                    [latitude_index, longitude_index]
                } else {
                    [longitude_index, latitude_index]
                };
                let values = columns
                    .iter()
                    .zip(&planes)
                    .filter_map(|(name, plane)| plane.get(cell).map(|value| (name, *value)));
                if let Some(record) = location_record(latitude, longitude, &plane_facets, values) {
                    batches.push(record)?;
                }
            }
        }
    }
    Ok(())
}

/// The hyperslab of one latitude-longitude plane: every latitude and longitude, and the plane's index
/// along each other dimension.
fn plane_selection(layout: &GridLayout<'_>, indices: &[usize]) -> Result<NcSliceInfo, NetCdfIngestError> {
    let selections = layout
        .dimensions
        .iter()
        .enumerate()
        .map(
            |(position, dimension)| match layout.facets.iter().zip(indices).find(|(axis, _)| axis.position == position) {
                Some((_, &index)) => u64::try_from(index).map(NcSliceInfoElem::Index).map_err(|_| too_long(dimension)),
                None => Ok(NcSliceInfoElem::Slice {
                    start: 0,
                    end: dimension.size,
                    step: 1,
                }),
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NcSliceInfo { selections })
}

#[cfg(test)]
mod tests {
    use crate::{
        netcdf::{
            fixture::{coordinate, variable, write},
            grid::{plane::ingest_grid_group, variables::grid_groups},
            nonexistent_dates::NonexistentDateLog,
        },
        record_batches::RecordBatches,
    };
    use cassiopeia_common::{channel::ChannelSender, signal::Signal};
    use netcdf_reader::NcFile;
    use netcdf_writer::{NcFileBuilder, NcWriteFormat};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{num::NonZeroU64, sync::mpsc::sync_channel};
    use temp_dir::TempDir;

    #[test]
    fn each_cell_of_each_plane_is_a_record_and_the_planes_time_dates_the_gregorian_calendar_lacks_are_logged() {
        let mut builder = NcFileBuilder::new();
        let time = coordinate(
            &mut builder,
            "time",
            &[29.0, 59.0],
            &[("standard_name", "time"), ("units", "days since 2026-01-01"), ("calendar", "360_day")],
        );
        let latitude = coordinate(&mut builder, "lat", &[46.0], &[("units", "degrees_north")]);
        let longitude = coordinate(&mut builder, "lon", &[14.0, 14.5], &[("units", "degrees_east")]);
        variable(
            &mut builder,
            "tas",
            &[time, latitude, longitude],
            &[280.0, 281.0, 282.0, 283.0],
            &[("units", "K")],
        );
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("grid.nc");
        write(&builder, &path, NcWriteFormat::Classic);
        let file = NcFile::open(&path).unwrap();
        let groups = grid_groups(file.root_group().unwrap()).unwrap();

        let (sender, receiver) = sync_channel(8);
        let sender = ChannelSender::bounded(sender);
        let mut nonexistent = NonexistentDateLog::default();
        let mut batches = RecordBatches::new(&sender, 16);
        ingest_grid_group(&file, &groups[0], &mut nonexistent, &mut batches).unwrap();
        batches.finish().unwrap();
        drop(sender);

        let records: Vec<Value> = receiver
            .iter()
            .flat_map(|signal| match signal {
                Signal::Data(records) => records,
                Signal::Start | Signal::Stop | Signal::Meta(_) | Signal::Error(_) => panic!("expected a data signal"),
            })
            .map(|record| Value::Object(record.into_data()))
            .collect();
        assert_eq!(
            records,
            [
                json!({"latitude": 46.0, "longitude": 14.0, "time": "2026-01-30T00:00:00+00:00", "tas": 280.0}),
                json!({"latitude": 46.0, "longitude": 14.5, "time": "2026-01-30T00:00:00+00:00", "tas": 281.0}),
                json!({"latitude": 46.0, "longitude": 14.0, "time": "2026-02-30T00:00:00+00:00", "tas": 282.0}),
                json!({"latitude": 46.0, "longitude": 14.5, "time": "2026-02-30T00:00:00+00:00", "tas": 283.0}),
            ]
        );
        let logged = nonexistent.variables();
        assert_eq!(logged.len(), 1);
        assert_eq!((logged[0].first.as_str(), logged[0].count), ("2026-02-30T00:00:00+00:00", NonZeroU64::MIN));
    }
}
