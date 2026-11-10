use crate::netcdf::{
    cf_attribute::CfAttribute,
    decode::Decode,
    dimension_length::dimension_length,
    error::NetCdfIngestError,
    time_series::stations::TimeSeriesCoordinates,
    value_kind::value_dimensions,
};
use netcdf_reader::{NcDimension, NcFile, NcGroup, NcVariable};
use num_traits::ToPrimitive;

/// One observation of a time series: the station it belongs to and its place along the element
/// dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    /// The station's index along the instance dimension; 0 in the single layout, which has none.
    pub station: usize,
    /// The observation's index along the element dimension.
    pub element: usize,
}

/// The dimensions a `timeSeries` file's observations range over, and every observation's place in
/// them, resolved from whichever of the five CF 1.11 Appendix H.2 layouts the file uses.
///
/// Every variable of the file is indexed through these two dimensions alone: a station variable
/// along the instance dimension, an observation variable along the element dimension, the station
/// dimension, or both. So once the layout is known, one observation's values are found the same way
/// in all five layouts. A further dimension of length one, such as the single `HEIGHT`, `LATITUDE` or
/// `LONGITUDE` an `OceanSITES` station file declares, holds one value for every observation, so it is
/// indexed at zero rather than refused.
#[derive(Debug, Clone)]
pub struct ObservationLayout<'f> {
    instance: Option<&'f NcDimension>,
    element: &'f NcDimension,
    observations: Vec<Observation>,
}

impl<'f> ObservationLayout<'f> {
    /// Detects the layout of the root group's time series (CF 1.11 §9.3):
    ///
    /// - contiguous ragged: a count variable carries `sample_dimension`, and each station's
    ///   observations are the next `count` elements;
    /// - indexed ragged: an index variable carries `instance_dimension`, and each element names its
    ///   station;
    /// - single: the latitude is a scalar, and the time runs along the element dimension;
    /// - orthogonal multidimensional: the latitude runs along the station dimension, and the time
    ///   along a dimension of its own shared by every station;
    /// - incomplete multidimensional: the time runs along the station dimension and the element
    ///   dimension, padded with fill values where a station has fewer observations.
    ///
    /// # Errors
    ///
    /// Returns [`NetCdfIngestError::InvalidRaggedArray`] when a count or index points outside its
    /// dimension, [`NetCdfIngestError::UnrecognizedTimeSeriesLayout`] when the coordinates fit no
    /// layout, and [`NetCdfIngestError::Read`] when a count or index variable cannot be read.
    pub fn detect(file: &NcFile, root: &'f NcGroup, coordinates: &TimeSeriesCoordinates<'f>) -> Result<ObservationLayout<'f>, NetCdfIngestError> {
        let carrying = |attribute: CfAttribute| {
            root.variables
                .iter()
                .find_map(|variable| attribute.text(variable.attributes()).map(|dimension| (variable, dimension)))
        };
        if let Some((counts, sample_dimension)) = carrying(CfAttribute::SampleDimension) {
            return contiguous(file, root, counts, sample_dimension);
        }
        if let Some((index, instance_dimension)) = carrying(CfAttribute::InstanceDimension) {
            return indexed(file, root, index, instance_dimension);
        }
        multidimensional(root, coordinates)
    }

    /// Every observation, in file order.
    #[must_use]
    pub fn observations(&self) -> &[Observation] {
        &self.observations
    }

    /// Whether `dimensions` are all the instance dimension, the element dimension or of length one, so
    /// every observation has one value in a variable over them.
    #[must_use]
    pub fn places(&self, dimensions: &[NcDimension]) -> bool {
        dimensions
            .iter()
            .all(|dimension| self.is_instance(dimension) || self.is_element(dimension) || dimension.size == 1)
    }

    /// Whether `dimensions` include the element dimension, as an observation variable's do.
    #[must_use]
    pub fn varies_by_observation(&self, dimensions: &[NcDimension]) -> bool {
        dimensions.iter().any(|dimension| self.is_element(dimension))
    }

    /// The row-major position of `observation`'s value in a variable over `dimensions`, or `None` when
    /// the layout does not place the variable.
    #[must_use]
    pub fn flat_index(&self, dimensions: &[NcDimension], observation: Observation) -> Option<usize> {
        dimensions.iter().try_fold(0_usize, |flat, dimension| {
            let index = if self.is_instance(dimension) {
                observation.station
            } else if self.is_element(dimension) {
                observation.element
            } else if dimension.size == 1 {
                0
            } else {
                return None;
            };
            flat.checked_mul(usize::try_from(dimension.size).ok()?)?.checked_add(index)
        })
    }

    fn is_instance(&self, dimension: &NcDimension) -> bool {
        self.instance.is_some_and(|instance| instance.name == dimension.name)
    }

    fn is_element(&self, dimension: &NcDimension) -> bool {
        self.element.name == dimension.name
    }
}

/// The contiguous ragged layout (CF 1.11 §9.3.3): station `i`'s observations are the `counts[i]`
/// elements following those of the stations before it. Elements past the last station's run belong to
/// no station, so they are skipped with a warning.
fn contiguous<'f>(file: &NcFile, root: &'f NcGroup, counts: &'f NcVariable, sample_dimension: &str) -> Result<ObservationLayout<'f>, NetCdfIngestError> {
    let invalid = || NetCdfIngestError::InvalidRaggedArray {
        variable: counts.name().to_owned(),
    };
    let [instance] = counts.dimensions() else {
        return Err(invalid());
    };
    let element = root
        .dimensions
        .iter()
        .find(|dimension| dimension.name == sample_dimension)
        .ok_or_else(invalid)?;
    let element_size = dimension_length(element)?;

    let mut observations = Vec::new();
    let mut start = 0_usize;
    for (station, count) in Decode::Unpack.read(file, counts)?.into_iter().enumerate() {
        let end = whole(count)
            .and_then(|count| start.checked_add(count))
            .filter(|end| *end <= element_size)
            .ok_or_else(invalid)?;
        observations.extend((start..end).map(|element| Observation { station, element }));
        start = end;
    }
    if start < element_size {
        tracing::warn!(
            variable = counts.name(),
            skipped = element_size - start,
            "NetCDF contiguous ragged array: elements past the last station's count belong to no station and are skipped"
        );
    }
    Ok(ObservationLayout {
        instance: Some(instance),
        element,
        observations,
    })
}

/// The indexed ragged layout (CF 1.11 §9.3.4): element `j` belongs to station `index[j]`. An element
/// whose index is masked belongs to no station and is no observation.
fn indexed<'f>(file: &NcFile, root: &'f NcGroup, index: &'f NcVariable, instance_dimension: &str) -> Result<ObservationLayout<'f>, NetCdfIngestError> {
    let invalid = || NetCdfIngestError::InvalidRaggedArray {
        variable: index.name().to_owned(),
    };
    let [element] = index.dimensions() else {
        return Err(invalid());
    };
    let instance = root
        .dimensions
        .iter()
        .find(|dimension| dimension.name == instance_dimension)
        .ok_or_else(invalid)?;
    let instance_size = dimension_length(instance)?;

    let mut observations = Vec::new();
    for (element, station) in Decode::MaskAndUnpack.read(file, index)?.into_iter().enumerate() {
        if station.is_nan() {
            continue;
        }
        let station = whole(station).filter(|station| *station < instance_size).ok_or_else(invalid)?;
        observations.push(Observation { station, element });
    }
    Ok(ObservationLayout {
        instance: Some(instance),
        element,
        observations,
    })
}

/// The single, orthogonal and incomplete multidimensional layouts (CF 1.11 §9.3.1, §9.3.2), told
/// apart by the shapes of the latitude and time coordinates.
///
/// A station latitude and a time over a dimension of its own look the same in the orthogonal layout
/// and in a ragged one whose count or index variable lacks its `sample_dimension` or
/// `instance_dimension` attribute. Read as orthogonal, the ragged file would repeat every observation
/// at every station, so the orthogonal layout of more than one station is accepted only when some
/// variable ranges over both the station and the time dimension, as its data variables do.
fn multidimensional<'f>(root: &'f NcGroup, coordinates: &TimeSeriesCoordinates<'f>) -> Result<ObservationLayout<'f>, NetCdfIngestError> {
    let time = value_dimensions(coordinates.time);
    let (instance, element) = match (value_dimensions(coordinates.latitude), time) {
        ([], [element]) => (None, element),
        ([instance], [element]) if instance.name != element.name => {
            if instance.size > 1 && !spans_both(root, instance, element) {
                return Err(NetCdfIngestError::UnrecognizedTimeSeriesLayout);
            }
            (Some(instance), element)
        }
        ([instance], [first, second]) if first.name == instance.name => (Some(instance), second),
        ([instance], [first, second]) if second.name == instance.name => (Some(instance), first),
        (_, _) => return Err(NetCdfIngestError::UnrecognizedTimeSeriesLayout),
    };

    let element_size = dimension_length(element)?;
    let stations = instance.map_or(Ok(1), dimension_length)?;
    let observations = (0..stations)
        .flat_map(|station| (0..element_size).map(move |element| Observation { station, element }))
        .collect();
    Ok(ObservationLayout {
        instance,
        element,
        observations,
    })
}

/// Whether some variable of `root` ranges over both `instance` and `element`.
fn spans_both(root: &NcGroup, instance: &NcDimension, element: &NcDimension) -> bool {
    root.variables.iter().any(|variable| {
        let dimensions = value_dimensions(variable);
        let has = |wanted: &NcDimension| dimensions.iter().any(|dimension| dimension.name == wanted.name);
        has(instance) && has(element)
    })
}

/// A count or index read as a float, when it is a non-negative whole number.
fn whole(value: f64) -> Option<usize> {
    (value.fract() == 0.0).then(|| value.to_usize()).flatten()
}

#[cfg(test)]
mod tests {
    use crate::netcdf::time_series::layout::whole;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_whole_non_negative_float_is_a_count() {
        assert_eq!(whole(3.0), Some(3));
        assert_eq!(whole(0.0), Some(0));
    }

    #[test]
    fn a_fractional_negative_or_missing_float_is_no_count() {
        assert_eq!(whole(2.5), None);
        assert_eq!(whole(-1.0), None);
        assert_eq!(whole(f64::NAN), None);
        assert_eq!(whole(f64::INFINITY), None);
    }
}
