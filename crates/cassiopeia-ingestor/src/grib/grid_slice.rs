use crate::{
    error::IngestorError,
    grib::field::DecodedField,
    location_record::{
        facets::LocationFacets,
        level::{Level, LevelGroupKey},
        parameter_name::ParameterName,
        record::location_record,
    },
    record_batches::RecordBatches,
};
use cassiopeia_common::{channel::ChannelSender, signal::Signal};
use cassiopeia_ir::record::Record;
use indexmap::{IndexMap, map::Entry};

/// Identity of a grid slice: GRIB fields sharing all of these describe the same locations at the same
/// level and time, so their parameters pivot into columns of one shared set of records.
///
/// The grid is identified by its [`DecodedField::grid`] bytes: two fields with byte-equal grid
/// identity have identical geometry and cell ordering, so their values line up cell for cell. Level
/// and the two times complete the key: the same parameter at another level or forecast step is a
/// distinct observation, not another column of the same record. The level is held as its float-safe
/// [`LevelGroupKey`] projection so a floating-point level value never enters a hashed key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GridSliceKey {
    grid: Vec<u8>,
    level: Option<LevelGroupKey>,
    reference_time: Option<String>,
    forecast_time: Option<String>,
}

impl GridSliceKey {
    /// Builds a slice key from the grid identity bytes, the level grouping key, and the two times.
    #[must_use]
    pub const fn new(grid: Vec<u8>, level: Option<LevelGroupKey>, reference_time: Option<String>, forecast_time: Option<String>) -> GridSliceKey {
        GridSliceKey {
            grid,
            level,
            reference_time,
            forecast_time,
        }
    }
}

/// Accumulates every decoded field, groups them into grid slices, and flushes the pivoted records.
///
/// A GRIB file stores one field per parameter, but a slice's parameters need not be adjacent in the
/// stream, so fields accumulate until the whole file is read before any record is emitted. The map
/// keeps first-seen order so records emit deterministically. This grouping is edition-agnostic: both
/// decoders feed it [`DecodedField`]s and it never learns which edition produced them.
#[derive(Default)]
pub struct GridSliceSet {
    slices: IndexMap<GridSliceKey, GridSlice>,
}

impl GridSliceSet {
    /// Creates an empty slice set.
    #[must_use]
    pub fn new() -> GridSliceSet {
        GridSliceSet::default()
    }

    /// Folds one decoded field into its slice, opening a new slice or adding a column to an existing
    /// one. Fields sharing a grid, level, and time pivot into columns of the same location records.
    pub fn add(&mut self, field: DecodedField) {
        let level_key = field.level.as_ref().map(Level::group_key);
        let key = GridSliceKey::new(field.grid, level_key, field.reference_time.clone(), field.forecast_time.clone());
        match self.slices.entry(key) {
            Entry::Occupied(mut occupied) => occupied.get_mut().add_parameter(field.parameter, field.values),
            Entry::Vacant(vacant) => {
                let mut slice = GridSlice::new(field.latlons, field.level, field.reference_time, field.forecast_time);
                slice.add_parameter(field.parameter, field.values);
                vacant.insert(slice);
            }
        }
    }

    /// Consumes the set, emitting its records to `sender` in batches of at most `batch_size`.
    ///
    /// # Errors
    ///
    /// Returns [`IngestorError::ChannelClosed`] if the receiver is dropped before all batches are sent.
    pub fn flush(self, batch_size: usize, sender: &ChannelSender<Signal<Vec<Record>, IngestorError>>) -> Result<(), IngestorError> {
        let mut batches = RecordBatches::new(sender, batch_size);
        self.slices
            .into_values()
            .flat_map(GridSlice::into_records)
            .try_for_each(|record| batches.push(record))?;
        batches.finish()
    }
}

/// Accumulates every parameter measured on one grid slice, then emits one record per grid cell with
/// each parameter as its own column.
///
/// This is the pivot at the heart of the wide model: a GRIB file stores one field per parameter, but
/// an NGSI-LD observation entity carries all co-located parameters as attributes of a single location.
/// The slice collects the parameter fields (all sharing the slice's grid and cell ordering) and
/// transposes them into per-location records.
pub struct GridSlice {
    latlons: Vec<(f64, f64)>,
    level: Option<Level>,
    reference_time: Option<String>,
    forecast_time: Option<String>,
    columns: Vec<(ParameterName, Vec<f64>)>,
}

impl GridSlice {
    /// Starts a slice from its grid's cell coordinates and shared level and times.
    #[must_use]
    pub const fn new(latlons: Vec<(f64, f64)>, level: Option<Level>, reference_time: Option<String>, forecast_time: Option<String>) -> GridSlice {
        GridSlice {
            latlons,
            level,
            reference_time,
            forecast_time,
            columns: Vec::new(),
        }
    }

    /// Adds one parameter's decoded values as a column.
    ///
    /// `values` is in the same cell order as the slice's `latlons`, so the i-th value belongs to the
    /// i-th coordinate. Every parameter added to a slice shares the slice's grid, so the lengths match.
    pub fn add_parameter(&mut self, name: ParameterName, values: Vec<f64>) {
        self.columns.push((name, values));
    }

    /// Consumes the slice, producing one record per grid cell that has at least one non-masked value.
    ///
    /// Bitmap-masked cells decode to NaN; a parameter is simply omitted from a record where its value
    /// is masked, and a cell where every parameter is masked yields no record at all. The level and the
    /// two times are the same for every cell of the slice, so they are resolved once into the facets
    /// each cell's record shares.
    #[must_use]
    pub fn into_records(self) -> Vec<Record> {
        let mut facets = LocationFacets::default();
        if let Some(level) = &self.level {
            facets = facets.with_level(level);
        }
        if let Some(reference_time) = self.reference_time {
            facets = facets.with_time(&ParameterName::from("referenceTime"), reference_time);
        }
        if let Some(forecast_time) = self.forecast_time {
            facets = facets.with_time(&ParameterName::from("forecastTime"), forecast_time);
        }
        let facets = facets;

        let columns = &self.columns;
        self.latlons
            .iter()
            .enumerate()
            .filter_map(|(index, &(latitude, longitude))| {
                let values = columns.iter().filter_map(|(name, values)| values.get(index).map(|value| (name, *value)));
                location_record(latitude, longitude, &facets, values)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        grib::{grid_slice::GridSlice, level::grib2_level},
        location_record::parameter_name::ParameterName,
    };
    use pretty_assertions::assert_eq;
    use serde_json::Value;

    fn slice() -> GridSlice {
        let mut slice = GridSlice::new(
            vec![(46.25, 14.0), (46.25, 14.5)],
            // Code Table 4.5 surface type 103 at 2 m normalizes to the height-above-ground level.
            grib2_level(103, 2.0),
            Some("2026-08-18T07:30:00+00:00".to_string()),
            Some("2026-08-18T13:30:00+00:00".to_string()),
        );
        slice.add_parameter(ParameterName::from("temperature"), vec![280.5, 281.0]);
        slice.add_parameter(ParameterName::from("precip"), vec![0.25, 0.5]);
        slice
    }

    #[test]
    fn a_cell_becomes_one_location_record_with_every_parameter_as_a_column() {
        let records = slice().into_records();
        assert_eq!(records.len(), 2);

        let first = records[0].data();
        assert_eq!(first.get("latitude").and_then(Value::as_f64), Some(46.25));
        assert_eq!(first.get("longitude").and_then(Value::as_f64), Some(14.0));
        assert_eq!(first.get("temperature").and_then(Value::as_f64), Some(280.5));
        assert_eq!(first.get("precip").and_then(Value::as_f64), Some(0.25));
        // The normalized level splits into a bare token and a whole-numbered value.
        assert_eq!(first.get("level_type").and_then(Value::as_str), Some("height_above_ground"));
        assert_eq!(first.get("level").and_then(Value::as_f64), Some(2.0));
        assert_eq!(first.get("level").and_then(Value::as_i64), Some(2));
        assert_eq!(first.get("forecastTime"), Some(&Value::String("2026-08-18T13:30:00+00:00".to_string())));
        // The wide model carries no collection: a GRIB file is one flat table of locations.
        assert_eq!(records[0].collection(), &None);
    }

    #[test]
    fn a_masked_parameter_is_omitted_but_the_cell_survives_on_its_other_values() {
        let mut slice = GridSlice::new(vec![(46.25, 14.0)], None, None, None);
        slice.add_parameter(ParameterName::from("temperature"), vec![f64::NAN]);
        slice.add_parameter(ParameterName::from("precip"), vec![0.5]);
        let records = slice.into_records();

        assert_eq!(records.len(), 1);
        let data = records[0].data();
        assert!(!data.contains_key("temperature"));
        assert_eq!(data.get("precip").and_then(Value::as_f64), Some(0.5));
    }

    #[test]
    fn a_cell_masked_in_every_parameter_produces_no_record() {
        let mut slice = GridSlice::new(vec![(46.2, 14.0)], None, None, None);
        slice.add_parameter(ParameterName::from("temperature"), vec![f64::NAN]);
        slice.add_parameter(ParameterName::from("precip"), vec![f64::NAN]);
        assert!(slice.into_records().is_empty());
    }
}
