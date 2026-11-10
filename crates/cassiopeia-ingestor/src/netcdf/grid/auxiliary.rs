use crate::{
    location_record::record::{LATITUDE, LONGITUDE},
    netcdf::{
        cf_attribute::CfAttribute,
        coordinate_facets::CoordinateKey,
        coordinate_role::CoordinateRole,
        grid::variables::FacetAxis,
        value_kind::{ValueKind, value_dimensions},
    },
};
use netcdf_reader::{NcDimension, NcGroup, NcVariable};

/// A coordinate a data variable names in its `coordinates` attribute that ranges over none of the
/// horizontal dimensions: a scalar coordinate, which CF 1.11 §5.7 makes equivalent to a size-one
/// coordinate variable and so applies to every cell, or an auxiliary coordinate over the plane
/// dimensions (CF 1.11 §5), such as a forecast reference time along the time dimension.
#[derive(Debug, Clone)]
pub struct AuxiliaryAxis<'f> {
    /// The coordinate variable.
    pub coordinate: &'f NcVariable,
    /// The positions, among the layout's facet axes, of the dimensions the coordinate ranges over, in
    /// the coordinate's own dimension order; empty for a scalar coordinate.
    pub facets: Vec<usize>,
    /// Where the coordinate's value lands in each record.
    pub key: CoordinateKey,
}

impl AuxiliaryAxis<'_> {
    /// The row-major position of the coordinate's value for the plane at `indices`, one index per
    /// facet axis of the layout.
    #[must_use]
    pub fn index(&self, indices: &[usize]) -> Option<usize> {
        value_dimensions(self.coordinate)
            .iter()
            .zip(&self.facets)
            .try_fold(0_usize, |flat, (dimension, facet)| {
                flat.checked_mul(usize::try_from(dimension.size).ok()?)?.checked_add(*indices.get(*facet)?)
            })
    }
}

/// The auxiliary axes of a data variable over `dimensions` whose non-horizontal dimensions are
/// `facets`, from the coordinates its `coordinates` attribute names.
///
/// A coordinate variable of one of the dimensions is already a facet axis, and a coordinate over a
/// horizontal dimension, such as a two-dimensional latitude, locates cells rather than planes, so
/// neither becomes an auxiliary axis. Each auxiliary coordinate takes the key its role gives it when
/// that key is still free, so a scalar `time` becomes the record's `time` and a scalar `height` its
/// level; a second time or vertical coordinate, whose key a dimension already took, falls back to its
/// own name, and a coordinate whose own name is taken too is skipped with a warning.
#[must_use]
pub fn auxiliary_axes<'f>(root: &'f NcGroup, variable: &NcVariable, dimensions: &[NcDimension], facets: &[FacetAxis<'_>]) -> Vec<AuxiliaryAxis<'f>> {
    let mut auxiliaries: Vec<AuxiliaryAxis<'f>> = Vec::new();
    for name in CfAttribute::Coordinates.names(variable.attributes()) {
        let Some(coordinate) = root.variable(name) else {
            tracing::warn!(
                variable = variable.name(),
                coordinate = name,
                "NetCDF coordinate skipped: no variable of that name exists"
            );
            continue;
        };
        let listed = auxiliaries.iter().any(|auxiliary| auxiliary.coordinate.name() == coordinate.name());
        if listed || coordinate.is_coordinate_variable() {
            continue;
        }
        if ValueKind::of(coordinate.dtype()) == ValueKind::UserDefined {
            tracing::warn!(coordinate = coordinate.name(), "NetCDF coordinate skipped: it holds a user-defined type");
            continue;
        }
        let Some(positions) = facet_positions(coordinate, dimensions, facets) else {
            continue;
        };
        let candidates = [
            CoordinateKey::of(coordinate, CoordinateRole::of(coordinate.attributes())),
            CoordinateKey::of(coordinate, CoordinateRole::Other),
        ];
        let Some(key) = candidates.into_iter().find(|key| !is_taken(key, coordinate, facets, &auxiliaries)) else {
            tracing::warn!(coordinate = coordinate.name(), "NetCDF coordinate skipped: its record key is already taken");
            continue;
        };
        auxiliaries.push(AuxiliaryAxis {
            coordinate,
            facets: positions,
            key,
        });
    }
    auxiliaries
}

/// The facet axis positions of the dimensions `coordinate` ranges over, or `None` when one of them is
/// not a facet dimension of the variable.
fn facet_positions(coordinate: &NcVariable, dimensions: &[NcDimension], facets: &[FacetAxis<'_>]) -> Option<Vec<usize>> {
    value_dimensions(coordinate)
        .iter()
        .map(|dimension| {
            facets.iter().position(|facet| {
                dimensions
                    .get(facet.position)
                    .is_some_and(|facet_dimension| facet_dimension.name == dimension.name)
            })
        })
        .collect()
}

/// Whether a record key `candidate` would fill is already filled by the horizontal position, a facet
/// axis or an earlier auxiliary axis.
fn is_taken(candidate: &CoordinateKey, coordinate: &NcVariable, facets: &[FacetAxis<'_>], auxiliaries: &[AuxiliaryAxis<'_>]) -> bool {
    let wanted = candidate.record_keys(coordinate);
    let facet_keys = facets.iter().flat_map(|facet| facet.key.record_keys(facet.coordinate));
    let auxiliary_keys = auxiliaries.iter().flat_map(|auxiliary| auxiliary.key.record_keys(auxiliary.coordinate));
    [LATITUDE, LONGITUDE]
        .into_iter()
        .chain(facet_keys.chain(auxiliary_keys).map(|taken| taken.key))
        .any(|taken| wanted.iter().any(|key| key.key == taken))
}

#[cfg(test)]
mod tests {
    use crate::netcdf::fixture::{coordinate, ingest, variable, write};
    use cassiopeia_ir::record::Record;
    use netcdf_writer::{DimensionId, NcFileBuilder, NcWriteFormat};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use temp_dir::TempDir;

    const HOURS: (&str, &str) = ("units", "hours since 2026-01-01");

    fn ingested(builder: &NcFileBuilder) -> Vec<Record> {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("grid.nc");
        write(builder, &path, NcWriteFormat::Classic);
        ingest(&path).unwrap()
    }

    fn single_cell(builder: &mut NcFileBuilder) -> [DimensionId; 2] {
        [
            coordinate(builder, "lat", &[46.0], &[("units", "degrees_north")]),
            coordinate(builder, "lon", &[14.0], &[("units", "degrees_east")]),
        ]
    }

    fn field<'r>(record: &'r Record, key: &str) -> Option<&'r Value> {
        record.data().get(key)
    }

    #[test]
    fn a_scalar_time_coordinate_becomes_the_time_of_every_record() {
        let mut builder = NcFileBuilder::new();
        let cell = single_cell(&mut builder);
        variable(&mut builder, "time", &[], &[6.0], &[("standard_name", "time"), HOURS]);
        variable(&mut builder, "t", &cell, &[280.0], &[("coordinates", "time")]);

        let records = ingested(&builder);
        assert_eq!(field(&records[0], "time"), Some(&json!("2026-01-01T06:00:00+00:00")));
    }

    #[test]
    fn variables_told_apart_only_by_a_scalar_height_keep_records_of_their_own() {
        let mut builder = NcFileBuilder::new();
        let cell = single_cell(&mut builder);
        let height = [("standard_name", "height"), ("units", "m"), ("positive", "up")];
        variable(&mut builder, "height_2m", &[], &[2.0], &height);
        variable(&mut builder, "height_10m", &[], &[10.0], &height);
        variable(&mut builder, "tas", &cell, &[280.0], &[("coordinates", "height_2m")]);
        variable(&mut builder, "uas", &cell, &[3.5], &[("coordinates", "height_10m")]);

        let records = ingested(&builder);
        assert_eq!(records.len(), 2);
        assert_eq!(
            Value::Object(records[0].data().clone()),
            json!({"latitude": 46.0, "longitude": 14.0, "level_type": "height_above_ground", "level": 2, "tas": 280.0})
        );
        assert_eq!(
            Value::Object(records[1].data().clone()),
            json!({"latitude": 46.0, "longitude": 14.0, "level_type": "height_above_ground", "level": 10, "uas": 3.5})
        );
    }

    #[test]
    fn an_auxiliary_reference_time_along_the_time_dimension_stamps_each_plane() {
        let mut builder = NcFileBuilder::new();
        let time = coordinate(&mut builder, "time", &[6.0, 12.0], &[("standard_name", "time"), HOURS]);
        let [latitude, longitude] = single_cell(&mut builder);
        variable(
            &mut builder,
            "reftime",
            &[time],
            &[0.0, 6.0],
            &[("standard_name", "forecast_reference_time"), HOURS],
        );
        variable(
            &mut builder,
            "t",
            &[time, latitude, longitude],
            &[280.0, 281.0],
            &[("coordinates", "reftime time lat lon")],
        );

        let records = ingested(&builder);
        assert_eq!(field(&records[0], "reftime"), Some(&json!("2026-01-01T00:00:00+00:00")));
        assert_eq!(field(&records[1], "reftime"), Some(&json!("2026-01-01T06:00:00+00:00")));
        assert_eq!(field(&records[1], "time"), Some(&json!("2026-01-01T12:00:00+00:00")));
    }

    #[test]
    fn a_second_time_coordinate_falls_back_to_its_own_name() {
        let mut builder = NcFileBuilder::new();
        let time = coordinate(&mut builder, "time", &[6.0], &[("standard_name", "time"), HOURS]);
        let [latitude, longitude] = single_cell(&mut builder);
        variable(&mut builder, "valid", &[], &[7.0], &[("standard_name", "time"), HOURS]);
        variable(&mut builder, "t", &[time, latitude, longitude], &[280.0], &[("coordinates", "valid")]);

        let record = &ingested(&builder)[0];
        assert_eq!(field(record, "time"), Some(&json!("2026-01-01T06:00:00+00:00")));
        assert_eq!(field(record, "valid"), Some(&json!("2026-01-01T07:00:00+00:00")));
    }

    #[test]
    fn a_listed_coordinate_over_a_horizontal_dimension_or_missing_from_the_file_adds_nothing() {
        let mut builder = NcFileBuilder::new();
        let [latitude, longitude] = single_cell(&mut builder);
        variable(&mut builder, "orography", &[latitude, longitude], &[300.0], &[("units", "m")]);
        variable(&mut builder, "t", &[latitude, longitude], &[280.0], &[("coordinates", "orography absent")]);

        let records = ingested(&builder);
        let temperature = records.iter().find(|record| record.data().contains_key("t")).unwrap();
        assert_eq!(
            Value::Object(temperature.data().clone()),
            json!({"latitude": 46.0, "longitude": 14.0, "t": 280.0, "orography": 300.0})
        );
    }
}
