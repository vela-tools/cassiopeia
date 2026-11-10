use crate::netcdf::{cf_attribute::CfAttribute, pressure_unit::PressureUnit, standard_name::StandardName, time_axis::declares_epoch};
use netcdf_reader::NcAttribute;
use strum::Display;

/// The CF 1.11 §4.1 units that identify a latitude coordinate.
const LATITUDE_UNITS: [&str; 6] = ["degrees_north", "degree_north", "degree_N", "degrees_N", "degreeN", "degreesN"];

/// The CF 1.11 §4.2 units that identify a longitude coordinate.
const LONGITUDE_UNITS: [&str; 6] = ["degrees_east", "degree_east", "degree_E", "degrees_E", "degreeE", "degreesE"];

/// The role a coordinate variable plays in locating data, decided from its CF attributes alone.
///
/// A coordinate's name never decides its role: CF identifies coordinate types by their attributes
/// (CF 1.11 §4), and a variable called `lat` with no attributes is just a variable called `lat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "snake_case")]
pub enum CoordinateRole {
    /// Latitude, emitted as the record's `latitude`.
    Latitude,
    /// Longitude, emitted as the record's `longitude`.
    Longitude,
    /// Time, emitted as the record's `time`.
    Time,
    /// A vertical coordinate, emitted as the record's `level_type` and `level` when it maps onto a
    /// canonical level type.
    Vertical,
    /// The x axis of a projected or rotated grid, which this ingestor cannot place on the globe.
    ProjectedX,
    /// The y axis of a projected or rotated grid, which this ingestor cannot place on the globe.
    ProjectedY,
    /// Any other coordinate, emitted under its own variable name.
    Other,
}

impl CoordinateRole {
    /// Classifies a coordinate from its attributes, checking them in the order CF 1.11 §4 gives:
    /// `standard_name`, then `units`, then `axis`, then `positive`.
    ///
    /// Time units (`<unit> since <epoch>`) identify a time coordinate only when the variable declares
    /// no other standard name, so `forecast_reference_time` stays a coordinate of its own rather than
    /// competing with the valid time.
    #[must_use]
    pub fn of(attributes: &[NcAttribute]) -> CoordinateRole {
        let standard_name = CfAttribute::StandardName.text(attributes);
        StandardName::of(attributes)
            .map(from_standard_name)
            .or_else(|| CfAttribute::Units.text(attributes).and_then(|units| from_units(units, standard_name)))
            .or_else(|| CfAttribute::Axis.text(attributes).and_then(from_axis))
            .or_else(|| CfAttribute::Positive.text(attributes).and_then(from_positive))
            .unwrap_or(CoordinateRole::Other)
    }
}

/// The role a recognized standard name implies.
const fn from_standard_name(name: StandardName) -> CoordinateRole {
    match name {
        StandardName::Latitude => CoordinateRole::Latitude,
        StandardName::Longitude => CoordinateRole::Longitude,
        StandardName::Time => CoordinateRole::Time,
        StandardName::ProjectionXCoordinate | StandardName::ProjectionXAngularCoordinate | StandardName::GridLongitude => CoordinateRole::ProjectedX,
        StandardName::ProjectionYCoordinate | StandardName::ProjectionYAngularCoordinate | StandardName::GridLatitude => CoordinateRole::ProjectedY,
        StandardName::AirPressure
        | StandardName::Height
        | StandardName::Altitude
        | StandardName::HeightAboveMeanSeaLevel
        | StandardName::Depth
        | StandardName::DepthBelowGeoid
        | StandardName::HeightAboveGeopotentialDatum
        | StandardName::HeightAboveReferenceEllipsoid
        | StandardName::ModelLevelNumber
        | StandardName::AtmosphereLnPressureCoordinate
        | StandardName::AtmosphereSigmaCoordinate
        | StandardName::AtmosphereHybridSigmaPressureCoordinate
        | StandardName::AtmosphereHybridHeightCoordinate
        | StandardName::AtmosphereSleveCoordinate
        | StandardName::OceanSigmaCoordinate
        | StandardName::OceanSCoordinate
        | StandardName::OceanSCoordinateG1
        | StandardName::OceanSCoordinateG2
        | StandardName::OceanSigmaZCoordinate
        | StandardName::OceanDoubleSigmaCoordinate => CoordinateRole::Vertical,
    }
}

/// The role `units` imply: degrees north or east, a pressure, or an epoch when no standard name
/// already says what the variable is.
fn from_units(units: &str, standard_name: Option<&str>) -> Option<CoordinateRole> {
    if LATITUDE_UNITS.iter().any(|latitude| latitude.eq_ignore_ascii_case(units)) {
        Some(CoordinateRole::Latitude)
    } else if LONGITUDE_UNITS.iter().any(|longitude| longitude.eq_ignore_ascii_case(units)) {
        Some(CoordinateRole::Longitude)
    } else if PressureUnit::parse(units).is_some() {
        Some(CoordinateRole::Vertical)
    } else {
        (standard_name.is_none() && declares_epoch(units)).then_some(CoordinateRole::Time)
    }
}

/// The role an `axis` attribute implies. An `X` or `Y` axis without degree units is a projected axis.
fn from_axis(axis: &str) -> Option<CoordinateRole> {
    match axis.to_ascii_uppercase().as_str() {
        "X" => Some(CoordinateRole::ProjectedX),
        "Y" => Some(CoordinateRole::ProjectedY),
        "Z" => Some(CoordinateRole::Vertical),
        "T" => Some(CoordinateRole::Time),
        _other => None,
    }
}

/// The role a `positive` attribute implies: only a vertical coordinate has a direction.
fn from_positive(positive: &str) -> Option<CoordinateRole> {
    (positive.eq_ignore_ascii_case("up") || positive.eq_ignore_ascii_case("down")).then_some(CoordinateRole::Vertical)
}

#[cfg(test)]
mod tests {
    use crate::netcdf::coordinate_role::CoordinateRole;
    use netcdf_reader::{NcAttrValue, NcAttribute};
    use pretty_assertions::assert_eq;

    fn role(attributes: &[(&str, &str)]) -> CoordinateRole {
        let attributes: Vec<NcAttribute> = attributes
            .iter()
            .map(|(name, value)| NcAttribute {
                name: (*name).to_string(),
                value: NcAttrValue::Chars((*value).to_string()),
            })
            .collect();
        CoordinateRole::of(&attributes)
    }

    #[test]
    fn latitude_and_longitude_standard_names_decide_the_horizontal_roles() {
        assert_eq!(role(&[("standard_name", "latitude")]), CoordinateRole::Latitude);
        assert_eq!(role(&[("standard_name", "longitude")]), CoordinateRole::Longitude);
    }

    #[test]
    fn degree_units_decide_the_horizontal_roles() {
        for units in ["degrees_north", "degree_N", "degrees_N"] {
            assert_eq!(role(&[("units", units)]), CoordinateRole::Latitude, "{units}");
        }
        assert_eq!(role(&[("units", "degrees_east")]), CoordinateRole::Longitude);
    }

    #[test]
    fn a_metric_x_axis_and_a_projection_y_coordinate_are_projected() {
        assert_eq!(role(&[("axis", "X"), ("units", "m")]), CoordinateRole::ProjectedX);
        assert_eq!(role(&[("standard_name", "projection_y_coordinate")]), CoordinateRole::ProjectedY);
    }

    #[test]
    fn a_time_standard_name_or_epoch_units_decide_the_time_role() {
        assert_eq!(role(&[("standard_name", "time")]), CoordinateRole::Time);
        assert_eq!(role(&[("units", "hours since 1900-01-01")]), CoordinateRole::Time);
    }

    #[test]
    fn a_forecast_reference_time_with_epoch_units_is_another_coordinate() {
        let attributes = [("standard_name", "forecast_reference_time"), ("units", "hours since 1900-01-01")];
        assert_eq!(role(&attributes), CoordinateRole::Other);
    }

    #[test]
    fn a_z_axis_pressure_units_or_a_direction_decide_the_vertical_role() {
        assert_eq!(role(&[("axis", "Z")]), CoordinateRole::Vertical);
        assert_eq!(role(&[("units", "hPa")]), CoordinateRole::Vertical);
        assert_eq!(role(&[("positive", "down")]), CoordinateRole::Vertical);
    }

    #[test]
    fn a_variable_without_attributes_is_another_coordinate_whatever_its_name() {
        assert_eq!(role(&[]), CoordinateRole::Other);
    }
}
