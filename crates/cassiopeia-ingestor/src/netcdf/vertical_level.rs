use crate::{
    location_record::level::{Level, LevelType},
    netcdf::{cf_attribute::CfAttribute, depth_surface::DepthSurface, pressure_unit::PressureUnit, standard_name::StandardName},
};
use netcdf_reader::NcAttribute;

/// The UDUNITS spellings of the metre a dimensional vertical coordinate must use to map onto a level.
const METRE_UNITS: [&str; 5] = ["m", "meter", "meters", "metre", "metres"];

/// How a CF vertical coordinate's values become canonical levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAxis {
    /// A pressure coordinate, converted to the hectopascals every isobaric level is emitted in.
    Pressure(PressureUnit),
    /// A coordinate already in its level type's canonical unit: metres for heights and depths, the
    /// dimensionless coordinate value for sigma and hybrid levels.
    Native(LevelType),
}

impl VerticalAxis {
    /// The level mapping a vertical coordinate declares, or `None` when its values do not map onto a
    /// canonical level type; such a coordinate keeps its own name in the record.
    ///
    /// A standard name other than `air_pressure` decides alone: a parametric coordinate such as CAM's
    /// `atmosphere_hybrid_sigma_pressure_coordinate` declares `hPa` for its nominal `1000 * (a + b)`
    /// values, which are not pressures. Otherwise pressure units make an isobaric level, ahead of the
    /// `positive = "down"` a pressure coordinate usually also declares. Heights, altitudes and depths
    /// are recognized only in metres, the unit their level types are emitted in; any other length unit
    /// leaves the coordinate under its own name rather than converting it. A coordinate whose only
    /// vertical marker is `positive = "down"` in metres is a depth (CF 1.11 §4.3).
    ///
    /// A depth names no surface it is measured from, so `surface` is asked for the one the data on the
    /// coordinate tells, and only for a depth; a depth below no known surface keeps its own name.
    #[must_use]
    pub fn of(attributes: &[NcAttribute], surface: impl FnOnce() -> Option<DepthSurface>) -> Option<VerticalAxis> {
        match Declared::of(attributes)? {
            Declared::Level(axis) => Some(axis),
            Declared::Depth => surface().map(|surface| VerticalAxis::Native(surface.level_type())),
        }
    }

    /// The level a coordinate value names.
    #[must_use]
    pub fn level(self, value: f64) -> Level {
        match self {
            VerticalAxis::Pressure(unit) => Level::new(LevelType::Isobaric, Some(unit.to_hectopascals(value))),
            VerticalAxis::Native(level_type) => Level::new(level_type, Some(value)),
        }
    }
}

/// What a vertical coordinate's own attributes say its values are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Declared {
    /// Levels the attributes place on their own.
    Level(VerticalAxis),
    /// Depths in metres below a surface the attributes do not name.
    Depth,
}

impl Declared {
    /// What `attributes` declare, or `None` when they declare no level.
    fn of(attributes: &[NcAttribute]) -> Option<Declared> {
        let units = CfAttribute::Units.text(attributes);
        let length = match units {
            Some(units) if METRE_UNITS.contains(&units) => Length::Metres,
            Some(_) | None => Length::Other,
        };
        match StandardName::of(attributes) {
            Some(StandardName::AirPressure) | None => {
                let pressure = units.and_then(PressureUnit::parse).map(|unit| Declared::Level(VerticalAxis::Pressure(unit)));
                pressure.or_else(|| {
                    let down = CfAttribute::Positive
                        .text(attributes)
                        .is_some_and(|positive| positive.eq_ignore_ascii_case("down"));
                    (down && length == Length::Metres).then_some(Declared::Depth)
                })
            }
            Some(name) => Declared::of_standard_name(name, length),
        }
    }

    /// What a vertical standard name declares, given the coordinate's length unit.
    const fn of_standard_name(name: StandardName, length: Length) -> Option<Declared> {
        match (name, length) {
            (StandardName::Height, Length::Metres) => Some(Declared::native(LevelType::HeightAboveGround)),
            (StandardName::Altitude | StandardName::HeightAboveMeanSeaLevel, Length::Metres) => Some(Declared::native(LevelType::AltitudeAboveMeanSea)),
            (StandardName::Depth, Length::Metres) => Some(Declared::Depth),
            (StandardName::AtmosphereSigmaCoordinate, Length::Metres | Length::Other) => Some(Declared::native(LevelType::Sigma)),
            (StandardName::Height | StandardName::Altitude | StandardName::HeightAboveMeanSeaLevel | StandardName::Depth, Length::Other)
            | (
                StandardName::Latitude
                | StandardName::Longitude
                | StandardName::Time
                | StandardName::ProjectionXCoordinate
                | StandardName::ProjectionYCoordinate
                | StandardName::ProjectionXAngularCoordinate
                | StandardName::ProjectionYAngularCoordinate
                | StandardName::GridLongitude
                | StandardName::GridLatitude
                | StandardName::AirPressure
                | StandardName::DepthBelowGeoid
                | StandardName::HeightAboveGeopotentialDatum
                | StandardName::HeightAboveReferenceEllipsoid
                | StandardName::ModelLevelNumber
                | StandardName::AtmosphereLnPressureCoordinate
                | StandardName::AtmosphereHybridSigmaPressureCoordinate
                | StandardName::AtmosphereHybridHeightCoordinate
                | StandardName::AtmosphereSleveCoordinate
                | StandardName::OceanSigmaCoordinate
                | StandardName::OceanSCoordinate
                | StandardName::OceanSCoordinateG1
                | StandardName::OceanSCoordinateG2
                | StandardName::OceanSigmaZCoordinate
                | StandardName::OceanDoubleSigmaCoordinate,
                Length::Metres | Length::Other,
            ) => None,
        }
    }

    /// Levels of `level_type` in its canonical unit.
    const fn native(level_type: LevelType) -> Declared {
        Declared::Level(VerticalAxis::Native(level_type))
    }
}

/// Whether a dimensional vertical coordinate is in the metres its level type is emitted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Length {
    /// Metres.
    Metres,
    /// Another unit, or none.
    Other,
}

#[cfg(test)]
mod tests {
    use crate::netcdf::{depth_surface::DepthSurface, vertical_level::VerticalAxis};
    use netcdf_reader::{NcAttrValue, NcAttribute};
    use pretty_assertions::assert_eq;

    /// The level token and value a coordinate with `attributes` gives `value`, asking `surface` for the
    /// surface its data tells, or `None` when it maps onto no level.
    fn level_with(attributes: &[(&str, &str)], value: f64, surface: impl FnOnce() -> Option<DepthSurface>) -> Option<(String, Option<f64>)> {
        let attributes: Vec<NcAttribute> = attributes
            .iter()
            .map(|(name, text)| NcAttribute {
                name: (*name).to_string(),
                value: NcAttrValue::Chars((*text).to_string()),
            })
            .collect();
        VerticalAxis::of(&attributes, surface).map(|axis| {
            let level = axis.level(value);
            (level.type_token().to_string(), level.value())
        })
    }

    /// The level a depth coordinate with `attributes` gives `value` when its data tells `surface`.
    fn level_below(attributes: &[(&str, &str)], value: f64, surface: Option<DepthSurface>) -> Option<(String, Option<f64>)> {
        level_with(attributes, value, || surface)
    }

    /// The level a coordinate that is no depth gives `value`; asking for the surface of its data fails
    /// the test.
    fn level(attributes: &[(&str, &str)], value: f64) -> Option<(String, Option<f64>)> {
        level_with(attributes, value, || panic!("only a depth asks for the surface of its data"))
    }

    #[test]
    fn pressure_in_pascals_hectopascals_and_millibars_is_isobaric_in_hectopascals() {
        assert_eq!(level(&[("units", "Pa")], 85_000.0), Some(("isobaric".to_string(), Some(850.0))));
        assert_eq!(level(&[("units", "hPa")], 850.0), Some(("isobaric".to_string(), Some(850.0))));
        assert_eq!(level(&[("units", "mbar")], 500.0), Some(("isobaric".to_string(), Some(500.0))));
    }

    #[test]
    fn a_height_in_metres_is_a_height_above_ground() {
        let attributes = [("standard_name", "height"), ("units", "m")];
        assert_eq!(level(&attributes, 2.0), Some(("height_above_ground".to_string(), Some(2.0))));
    }

    #[test]
    fn an_altitude_in_metres_is_an_altitude_above_mean_sea() {
        let attributes = [("standard_name", "altitude"), ("units", "m")];
        assert_eq!(level(&attributes, 120.0), Some(("altitude_above_mean_sea".to_string(), Some(120.0))));
    }

    #[test]
    fn a_depth_in_metres_is_measured_below_the_surface_its_data_tells() {
        let attributes = [("standard_name", "depth"), ("positive", "down"), ("units", "m")];
        assert_eq!(
            level_below(&attributes, 0.05, Some(DepthSurface::Land)),
            Some(("depth_below_land".to_string(), Some(0.05)))
        );
        assert_eq!(
            level_below(&attributes, 0.494, Some(DepthSurface::Sea)),
            Some(("depth_below_sea".to_string(), Some(0.494)))
        );
    }

    #[test]
    fn a_downward_coordinate_in_metres_without_a_standard_name_is_measured_below_the_surface_its_data_tells() {
        let attributes = [("positive", "down"), ("units", "meters")];
        assert_eq!(
            level_below(&attributes, 0.5, Some(DepthSurface::Land)),
            Some(("depth_below_land".to_string(), Some(0.5)))
        );
        assert_eq!(
            level_below(&attributes, 10.0, Some(DepthSurface::Sea)),
            Some(("depth_below_sea".to_string(), Some(10.0)))
        );
    }

    #[test]
    fn a_depth_below_no_known_surface_maps_onto_no_level() {
        let depth = [("standard_name", "depth"), ("positive", "down"), ("units", "m")];
        assert_eq!(level_below(&depth, 0.05, None), None);
        assert_eq!(level_below(&[("positive", "down"), ("units", "m")], 0.05, None), None);
    }

    #[test]
    fn a_depth_in_centimetres_maps_onto_no_level() {
        assert_eq!(level(&[("standard_name", "depth"), ("positive", "down"), ("units", "cm")], 5.0), None);
    }

    #[test]
    fn a_depth_below_the_geoid_keeps_its_own_name_though_it_points_down_in_metres() {
        let geoid = [("standard_name", "depth_below_geoid"), ("positive", "down"), ("units", "m")];
        assert_eq!(level(&geoid, 120.0), None);
    }

    #[test]
    fn a_sigma_coordinate_keeps_its_dimensionless_value() {
        let sigma = [("standard_name", "atmosphere_sigma_coordinate")];
        assert_eq!(level(&sigma, 0.995), Some(("sigma".to_string(), Some(0.995))));
    }

    #[test]
    fn a_hybrid_coordinate_maps_onto_no_level_since_its_value_is_no_grib_model_level_number() {
        let hybrid = [("standard_name", "atmosphere_hybrid_sigma_pressure_coordinate")];
        assert_eq!(level(&hybrid, 0.5), None);
    }

    #[test]
    fn a_hybrid_coordinate_in_hectopascals_is_not_an_isobaric_level() {
        let cam = [
            ("standard_name", "atmosphere_hybrid_sigma_pressure_coordinate"),
            ("units", "hPa"),
            ("positive", "down"),
        ];
        assert_eq!(level(&cam, 992.556), None);
    }

    #[test]
    fn an_air_pressure_standard_name_with_pressure_units_is_isobaric() {
        let pressure = [("standard_name", "air_pressure"), ("units", "Pa")];
        assert_eq!(level(&pressure, 50_000.0), Some(("isobaric".to_string(), Some(500.0))));
    }

    #[test]
    fn a_model_level_number_maps_onto_no_level() {
        assert_eq!(level(&[("standard_name", "model_level_number"), ("axis", "Z")], 3.0), None);
    }

    #[test]
    fn a_height_in_kilometres_maps_onto_no_level() {
        assert_eq!(level(&[("standard_name", "height"), ("units", "km")], 2.0), None);
    }
}
