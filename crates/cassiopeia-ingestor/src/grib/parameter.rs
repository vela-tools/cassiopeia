use strum::IntoStaticStr;

/// A Cassiopeia-canonical GRIB parameter: a short, space-free, level-independent name shared by both
/// backends (grib-rs and ecCodes) and both editions, so one mapping works regardless of which decoder
/// read the file.
///
/// `snake_case` is forced, not a stylistic choice: the key is consumed bare in a Tera template
/// (`{{ wind_u }}`). A kebab name (`wind-u`) parses as subtraction and a spaced name forces
/// `this['...']` bracket access, so only `snake_case` or single-word tokens are usable without ceremony.
///
/// The vocabulary can only cover parameters present in a known table. GRIB stores numeric codes, never
/// an author-set name: the human name comes from a lookup table (WMO master tables, plus centre-local
/// tables). A parameter outside the curated set below has no canonical name and falls back to a
/// synthetic code key instead (`d{disc}c{cat}n{num}` for GRIB2, `g1t{table}c{centre}p{indicator}` for
/// GRIB1), which stays stable and distinct across backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum GribParameter {
    /// Air temperature (K).
    Temperature,
    /// Potential temperature (K).
    PotentialTemperature,
    /// Maximum temperature (K).
    MaxTemperature,
    /// Minimum temperature (K).
    MinTemperature,
    /// Dew point temperature (K).
    Dewpoint,
    /// Specific humidity (kg/kg).
    SpecificHumidity,
    /// Relative humidity (%).
    Humidity,
    /// Precipitable water (kg/m2).
    PrecipitableWater,
    /// Precipitation rate (kg/m2/s).
    PrecipitationRate,
    /// Total precipitation (kg/m2).
    Precip,
    /// Snow depth (m).
    SnowDepth,
    /// Water equivalent of accumulated snow depth (kg/m2).
    SnowWaterEquivalent,
    /// Wind direction, the compass bearing the wind blows from (degrees true).
    WindDir,
    /// Wind speed, the wind vector's magnitude (m/s).
    WindSpeed,
    /// Eastward (u) component of the wind (m/s).
    WindU,
    /// Northward (v) component of the wind (m/s).
    WindV,
    /// Vertical velocity in pressure coordinates (Pa/s).
    VerticalVelocity,
    /// Wind speed of a gust (m/s).
    Gust,
    /// Pressure (Pa).
    Pressure,
    /// Pressure reduced to mean sea level (Pa).
    PressureMsl,
    /// Pressure tendency (Pa/s).
    PressureTendency,
    /// Geopotential (m2/s2).
    Geopotential,
    /// Geopotential height (gpm).
    GeopotentialHeight,
    /// Total cloud cover (%).
    CloudCover,
    /// Low cloud cover (%).
    LowCloudCover,
    /// Medium cloud cover (%).
    MediumCloudCover,
    /// High cloud cover (%).
    HighCloudCover,
    /// Convective available potential energy (J/kg).
    Cape,
    /// Convective inhibition (J/kg).
    Cin,
    /// Visibility (m).
    Visibility,
}

impl GribParameter {
    /// Recognizes a parameter from its GRIB2 `(discipline, parameterCategory, parameterNumber)` triple.
    ///
    /// This is the level-independent identity carried natively on a GRIB2 message, so grib-rs and
    /// ecCodes both resolve GRIB2 through this one function and agree cell for cell. The triples are
    /// WMO Code Table 4.2 for discipline 0 (meteorological products). A triple outside the curated set
    /// returns `None`, and the caller falls back to a synthetic code key.
    const fn from_grib2(discipline: u8, category: u8, number: u8) -> Option<GribParameter> {
        match (discipline, category, number) {
            (0, 0, 0) => Some(GribParameter::Temperature),
            (0, 0, 2) => Some(GribParameter::PotentialTemperature),
            (0, 0, 4) => Some(GribParameter::MaxTemperature),
            (0, 0, 5) => Some(GribParameter::MinTemperature),
            (0, 0, 6) => Some(GribParameter::Dewpoint),
            (0, 1, 0) => Some(GribParameter::SpecificHumidity),
            (0, 1, 1) => Some(GribParameter::Humidity),
            (0, 1, 3) => Some(GribParameter::PrecipitableWater),
            (0, 1, 7) => Some(GribParameter::PrecipitationRate),
            (0, 1, 8) => Some(GribParameter::Precip),
            (0, 1, 11) => Some(GribParameter::SnowDepth),
            (0, 1, 13) => Some(GribParameter::SnowWaterEquivalent),
            (0, 2, 0) => Some(GribParameter::WindDir),
            (0, 2, 1) => Some(GribParameter::WindSpeed),
            (0, 2, 2) => Some(GribParameter::WindU),
            (0, 2, 3) => Some(GribParameter::WindV),
            (0, 2, 8) => Some(GribParameter::VerticalVelocity),
            (0, 2, 22) => Some(GribParameter::Gust),
            (0, 3, 0) => Some(GribParameter::Pressure),
            (0, 3, 1) => Some(GribParameter::PressureMsl),
            (0, 3, 2) => Some(GribParameter::PressureTendency),
            (0, 3, 4) => Some(GribParameter::Geopotential),
            (0, 3, 5) => Some(GribParameter::GeopotentialHeight),
            (0, 6, 1) => Some(GribParameter::CloudCover),
            (0, 6, 3) => Some(GribParameter::LowCloudCover),
            (0, 6, 4) => Some(GribParameter::MediumCloudCover),
            (0, 6, 5) => Some(GribParameter::HighCloudCover),
            (0, 7, 6) => Some(GribParameter::Cape),
            (0, 7, 7) => Some(GribParameter::Cin),
            (0, 19, 0) => Some(GribParameter::Visibility),
            _ => None,
        }
    }

    /// Recognizes a parameter from its GRIB1 `indicatorOfParameter`.
    ///
    /// The level-independent identity on a GRIB1 message is the WMO Code Table 2 number; the triple of
    /// GRIB2 does not exist here. These are the standard Table 2 numbers (centre-local tables normally
    /// keep 1..=127 for standard quantities), so a GRIB1 file and a GRIB2 file naming the same physical
    /// quantity resolve to the same [`GribParameter`]. An unregistered indicator returns `None`.
    const fn from_grib1(indicator_of_parameter: i64) -> Option<GribParameter> {
        match indicator_of_parameter {
            1 => Some(GribParameter::Pressure),
            2 => Some(GribParameter::PressureMsl),
            3 => Some(GribParameter::PressureTendency),
            6 => Some(GribParameter::Geopotential),
            7 => Some(GribParameter::GeopotentialHeight),
            11 => Some(GribParameter::Temperature),
            13 => Some(GribParameter::PotentialTemperature),
            15 => Some(GribParameter::MaxTemperature),
            16 => Some(GribParameter::MinTemperature),
            17 => Some(GribParameter::Dewpoint),
            20 => Some(GribParameter::Visibility),
            31 => Some(GribParameter::WindDir),
            32 => Some(GribParameter::WindSpeed),
            33 => Some(GribParameter::WindU),
            34 => Some(GribParameter::WindV),
            39 => Some(GribParameter::VerticalVelocity),
            51 => Some(GribParameter::SpecificHumidity),
            52 => Some(GribParameter::Humidity),
            54 => Some(GribParameter::PrecipitableWater),
            59 => Some(GribParameter::PrecipitationRate),
            61 => Some(GribParameter::Precip),
            65 => Some(GribParameter::SnowWaterEquivalent),
            66 => Some(GribParameter::SnowDepth),
            71 => Some(GribParameter::CloudCover),
            73 => Some(GribParameter::LowCloudCover),
            74 => Some(GribParameter::MediumCloudCover),
            75 => Some(GribParameter::HighCloudCover),
            156 => Some(GribParameter::Cin),
            157 => Some(GribParameter::Cape),
            180 => Some(GribParameter::Gust),
            _ => None,
        }
    }

    /// The canonical `snake_case` column key for this parameter.
    fn key(self) -> &'static str {
        self.into()
    }
}

/// The key under which a GRIB parameter's value is stored in a location record.
///
/// A newtype rather than a bare string so a parameter column key cannot be confused with an unrelated
/// string (a level token, a timestamp) as it flows through the pivot. It holds either a canonical
/// [`GribParameter`] key or, for a parameter outside the curated vocabulary, a synthetic code key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterName(String);

impl ParameterName {
    /// Returns the parameter name as a string slice, for use as a record column key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ParameterName {
    fn from(value: &str) -> ParameterName {
        ParameterName(value.to_string())
    }
}

/// Resolves the canonical column key for a GRIB2 parameter from its `(discipline, category, number)`
/// triple.
///
/// grib-rs and ecCodes both feed this same function for GRIB2, so a file decoded either way yields the
/// same key, including the identical `d{discipline}c{category}n{number}` synthetic key for a parameter
/// outside the curated vocabulary. The lookup itself is `snake_case`-serialized by `strum`.
#[must_use]
pub fn parameter_key_grib2(discipline: u8, category: u8, number: u8) -> ParameterName {
    GribParameter::from_grib2(discipline, category, number).map_or_else(
        || ParameterName::from(format!("d{discipline}c{category}n{number}").as_str()),
        |parameter| ParameterName::from(parameter.key()),
    )
}

/// Resolves the canonical column key for a GRIB1 parameter from its identity.
///
/// The `indicator_of_parameter` drives the canonical lookup; `table2_version` and `centre` complete
/// the synthetic `g1t{table}c{centre}p{indicator}` key used when the indicator is outside the curated
/// vocabulary, so a centre-local parameter stays stable and distinct.
#[must_use]
pub fn parameter_key_grib1(table2_version: i64, centre: i64, indicator: i64) -> ParameterName {
    GribParameter::from_grib1(indicator).map_or_else(
        || ParameterName::from(format!("g1t{table2_version}c{centre}p{indicator}").as_str()),
        |parameter| ParameterName::from(parameter.key()),
    )
}

#[cfg(test)]
mod tests {
    use crate::grib::parameter::{GribParameter, parameter_key_grib1, parameter_key_grib2};

    #[test]
    fn a_known_grib2_triple_resolves_to_its_canonical_key() {
        // WMO Code Table 4.2, discipline 0, category 0 (temperature), number 0.
        assert_eq!(parameter_key_grib2(0, 0, 0).as_str(), "temperature");
    }

    #[test]
    fn total_precipitation_resolves_to_the_canonical_precip_key() {
        // Code Table 4.2, discipline 0, category 1 (moisture), number 8.
        assert_eq!(parameter_key_grib2(0, 1, 8).as_str(), "precip");
    }

    #[test]
    fn the_u_wind_component_resolves_to_its_canonical_key() {
        // Code Table 4.2, discipline 0, category 2 (momentum), number 2.
        assert_eq!(parameter_key_grib2(0, 2, 2).as_str(), "wind_u");
    }

    #[test]
    fn a_multiword_parameter_serializes_to_snake_case() {
        // strum's snake_case rename is what keeps the key bare-templatable.
        assert_eq!(GribParameter::PressureMsl.key(), "pressure_msl");
        assert_eq!(GribParameter::GeopotentialHeight.key(), "geopotential_height");
    }

    #[test]
    fn an_unknown_grib2_triple_falls_back_to_a_synthetic_key() {
        assert_eq!(parameter_key_grib2(0, 99, 5).as_str(), "d0c99n5");
    }

    #[test]
    fn a_known_grib1_indicator_resolves_to_its_canonical_key() {
        // WMO GRIB1 Code Table 2: 11 = temperature, 33 = u-wind, 61 = total precipitation.
        assert_eq!(parameter_key_grib1(2, 7, 11).as_str(), "temperature");
        assert_eq!(parameter_key_grib1(2, 7, 33).as_str(), "wind_u");
        assert_eq!(parameter_key_grib1(2, 7, 61).as_str(), "precip");
    }

    #[test]
    fn an_unregistered_grib1_indicator_falls_back_to_a_synthetic_key() {
        assert_eq!(parameter_key_grib1(128, 250, 200).as_str(), "g1t128c250p200");
    }

    #[test]
    fn the_two_editions_agree_on_the_canonical_key_for_the_same_quantity() {
        // The whole point of the canonical layer: a GRIB2 file and a GRIB1 file naming air temperature
        // resolve to the same bare key, so one mapping works on either.
        assert_eq!(parameter_key_grib2(0, 0, 0).as_str(), parameter_key_grib1(2, 7, 11).as_str());
        assert_eq!(parameter_key_grib2(0, 0, 0).as_str(), "temperature");
    }
}
