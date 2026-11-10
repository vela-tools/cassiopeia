use crate::netcdf::cf_attribute::CfAttribute;
use netcdf_reader::NcAttribute;
use strum::EnumString;

/// The CF standard names (CF 1.11 §3.3, Appendix D) that identify a coordinate's role or vertical
/// level type. Any other standard name is not a coordinate this ingestor interprets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum StandardName {
    /// Latitude, positive north (CF 1.11 §4.1).
    Latitude,
    /// Longitude, positive east (CF 1.11 §4.2).
    Longitude,
    /// Time (CF 1.11 §4.4).
    Time,
    /// The x coordinate of a projected grid (CF 1.11 §5.6).
    #[strum(serialize = "projection_x_coordinate")]
    ProjectionXCoordinate,
    /// The y coordinate of a projected grid (CF 1.11 §5.6).
    #[strum(serialize = "projection_y_coordinate")]
    ProjectionYCoordinate,
    /// The x coordinate of a geostationary projection, as a scanning angle (CF 1.11 Appendix F).
    #[strum(serialize = "projection_x_angular_coordinate")]
    ProjectionXAngularCoordinate,
    /// The y coordinate of a geostationary projection, as a scanning angle (CF 1.11 Appendix F).
    #[strum(serialize = "projection_y_angular_coordinate")]
    ProjectionYAngularCoordinate,
    /// The longitude of a rotated-pole grid (CF 1.11 Appendix F).
    GridLongitude,
    /// The latitude of a rotated-pole grid (CF 1.11 Appendix F).
    GridLatitude,
    /// Air pressure, a dimensional vertical coordinate.
    AirPressure,
    /// Height above the surface, a dimensional vertical coordinate.
    Height,
    /// Altitude above the geoid, a dimensional vertical coordinate.
    Altitude,
    /// Height above mean sea level, the historical alias of `altitude`.
    HeightAboveMeanSeaLevel,
    /// Depth below a surface the name leaves unnamed, a dimensional vertical coordinate.
    Depth,
    /// Depth below the geoid, a dimensional vertical coordinate.
    DepthBelowGeoid,
    /// Height above the geopotential datum, a dimensional vertical coordinate.
    HeightAboveGeopotentialDatum,
    /// Height above the reference ellipsoid, a dimensional vertical coordinate.
    HeightAboveReferenceEllipsoid,
    /// A model level index.
    ModelLevelNumber,
    /// Atmosphere natural log pressure coordinate (CF 1.11 Appendix D).
    AtmosphereLnPressureCoordinate,
    /// Atmosphere sigma coordinate (CF 1.11 Appendix D).
    AtmosphereSigmaCoordinate,
    /// Atmosphere hybrid sigma pressure coordinate (CF 1.11 Appendix D).
    AtmosphereHybridSigmaPressureCoordinate,
    /// Atmosphere hybrid height coordinate (CF 1.11 Appendix D).
    AtmosphereHybridHeightCoordinate,
    /// Atmosphere smooth level vertical (SLEVE) coordinate (CF 1.11 Appendix D).
    AtmosphereSleveCoordinate,
    /// Ocean sigma coordinate (CF 1.11 Appendix D).
    OceanSigmaCoordinate,
    /// Ocean s-coordinate (CF 1.11 Appendix D).
    #[strum(serialize = "ocean_s_coordinate")]
    OceanSCoordinate,
    /// Ocean s-coordinate, generic form 1 (CF 1.11 Appendix D).
    #[strum(serialize = "ocean_s_coordinate_g1")]
    OceanSCoordinateG1,
    /// Ocean s-coordinate, generic form 2 (CF 1.11 Appendix D).
    #[strum(serialize = "ocean_s_coordinate_g2")]
    OceanSCoordinateG2,
    /// Ocean sigma over z coordinate (CF 1.11 Appendix D).
    #[strum(serialize = "ocean_sigma_z_coordinate")]
    OceanSigmaZCoordinate,
    /// Ocean double sigma coordinate (CF 1.11 Appendix D).
    OceanDoubleSigmaCoordinate,
}

impl StandardName {
    /// The recognized standard name `attributes` declare, if any.
    ///
    /// A standard name may carry a modifier after whitespace (`air_temperature standard_error`, CF 1.11
    /// §3.3), so only its first word is the name.
    #[must_use]
    pub fn of(attributes: &[NcAttribute]) -> Option<StandardName> {
        CfAttribute::StandardName.text(attributes)?.split_whitespace().next()?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::standard_name::StandardName;
    use netcdf_reader::{NcAttrValue, NcAttribute};
    use pretty_assertions::assert_eq;

    fn named(name: &str) -> Option<StandardName> {
        StandardName::of(&[NcAttribute {
            name: "standard_name".to_string(),
            value: NcAttrValue::Chars(name.to_string()),
        }])
    }

    #[test]
    fn multi_word_standard_names_parse_from_their_cf_spelling() {
        assert_eq!(named("projection_x_coordinate"), Some(StandardName::ProjectionXCoordinate));
        assert_eq!(
            named("atmosphere_hybrid_sigma_pressure_coordinate"),
            Some(StandardName::AtmosphereHybridSigmaPressureCoordinate)
        );
        assert_eq!(named("ocean_s_coordinate_g2"), Some(StandardName::OceanSCoordinateG2));
        assert_eq!(named("height_above_mean_sea_level"), Some(StandardName::HeightAboveMeanSeaLevel));
    }

    #[test]
    fn a_modifier_after_the_name_is_ignored() {
        assert_eq!(named("altitude status_flag"), Some(StandardName::Altitude));
    }

    #[test]
    fn a_standard_name_outside_the_set_is_not_recognized() {
        assert_eq!(named("air_temperature"), None);
        assert_eq!(named("forecast_reference_time"), None);
    }
}
