use crate::location_record::level::{Level, LevelType};

/// Code Table 4.5 surface type 255 means "missing": the field carries no vertical level to normalize.
const MISSING_SURFACE: u8 = 255;

/// Recognizes a level type from a GRIB2 Code Table 4.5 surface type. Returns `None` for a surface type
/// outside the curated set (including 255, "missing").
const fn grib2_level_type(surface_type: u8) -> Option<LevelType> {
    match surface_type {
        1 => Some(LevelType::Surface),
        2 => Some(LevelType::CloudBase),
        3 => Some(LevelType::CloudTop),
        4 => Some(LevelType::IsothermZero),
        6 => Some(LevelType::MaxWind),
        7 => Some(LevelType::Tropopause),
        8 => Some(LevelType::TopOfAtmosphere),
        10 => Some(LevelType::EntireAtmosphere),
        100 => Some(LevelType::Isobaric),
        101 => Some(LevelType::MeanSeaLevel),
        102 => Some(LevelType::AltitudeAboveMeanSea),
        103 => Some(LevelType::HeightAboveGround),
        104 => Some(LevelType::Sigma),
        105 => Some(LevelType::Hybrid),
        106 => Some(LevelType::DepthBelowLand),
        109 => Some(LevelType::PotentialVorticity),
        _ => None,
    }
}

/// Recognizes a level type from an ecCodes `typeOfLevel` string (both editions). Returns `None` for a
/// token outside the curated set.
fn eccodes_level_type(type_of_level: &str) -> Option<LevelType> {
    match type_of_level {
        "surface" => Some(LevelType::Surface),
        "cloudBase" => Some(LevelType::CloudBase),
        "cloudTop" => Some(LevelType::CloudTop),
        "isothermZero" => Some(LevelType::IsothermZero),
        "maxWind" => Some(LevelType::MaxWind),
        "tropopause" => Some(LevelType::Tropopause),
        "nominalTop" => Some(LevelType::TopOfAtmosphere),
        "entireAtmosphere" | "atmosphere" | "atmosphereSingleLayer" => Some(LevelType::EntireAtmosphere),
        "isobaricInhPa" | "isobaricInPa" => Some(LevelType::Isobaric),
        "meanSea" => Some(LevelType::MeanSeaLevel),
        "heightAboveSea" => Some(LevelType::AltitudeAboveMeanSea),
        "heightAboveGround" | "heightAboveGroundLayer" => Some(LevelType::HeightAboveGround),
        "sigma" | "sigmaLayer" => Some(LevelType::Sigma),
        "hybrid" | "hybridLayer" => Some(LevelType::Hybrid),
        "depthBelowLand" | "depthBelowLandLayer" => Some(LevelType::DepthBelowLand),
        "potentialVorticity" => Some(LevelType::PotentialVorticity),
        _ => None,
    }
}

/// Losslessly widens an ecCodes integer level to `f64`.
///
/// ecCodes reports the level as a native `long`, and a level value always fits well within `i32` (hPa,
/// metres, model indices), so widening through `i32` avoids a lossy `i64`-to-`f64` cast; an
/// out-of-range value (never seen in practice) simply drops to an absent level.
fn widen(level: i64) -> Option<f64> {
    i32::try_from(level).ok().map(f64::from)
}

/// Normalizes an ecCodes integer level to the canonical unit for its type.
///
/// Only isobaric levels need conversion: ecCodes reports `isobaricInPa` in pascals (converted to the
/// canonical hPa) and `isobaricInhPa` already in hectopascals. Heights and depths are metres as
/// reported.
fn eccodes_value(level_type: LevelType, type_of_level: &str, raw: i64) -> Option<f64> {
    let value = widen(raw)?;
    Some(if matches!(level_type, LevelType::Isobaric) && type_of_level == "isobaricInPa" {
        value / 100.0
    } else {
        value
    })
}

/// Normalizes a grib-rs GRIB2 level from a Code Table 4.5 surface type and its decoded value.
///
/// Returns `None` for surface type 255 ("missing"), which carries no level. grib-rs reports an isobaric
/// surface in pascals, converted to the canonical hPa here. A surface type outside the curated set
/// keeps the stable fallback token `sfc{surface_type}`.
#[must_use]
pub fn grib2_level(surface_type: u8, value: f64) -> Option<Level> {
    if surface_type == MISSING_SURFACE {
        return None;
    }
    Some(match grib2_level_type(surface_type) {
        Some(LevelType::Isobaric) => Level::new(LevelType::Isobaric, Some(value / 100.0)),
        Some(level_type) => Level::new(level_type, Some(value)),
        None => Level::unrecognized(format!("sfc{surface_type}"), Some(value)),
    })
}

/// Normalizes an ecCodes level (both editions) from a `typeOfLevel` string and integer level value.
///
/// ecCodes reports the value in the unit named by the `typeOfLevel` token, so `isobaricInPa` is
/// converted to the canonical hPa while `isobaricInhPa` is already there; heights and depths are
/// metres as reported. A `typeOfLevel` outside the curated set keeps the raw token, so those rare cases
/// may differ from the grib-rs fallback.
#[must_use]
pub fn eccodes_level(type_of_level: &str, level: Option<i64>) -> Level {
    match eccodes_level_type(type_of_level) {
        Some(level_type) => Level::new(level_type, level.and_then(|raw| eccodes_value(level_type, type_of_level, raw))),
        None => Level::unrecognized(type_of_level.to_owned(), level.and_then(widen)),
    }
}

#[cfg(test)]
mod tests {
    use crate::grib::level::{eccodes_level, grib2_level};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_height_surface_type_normalizes_to_the_canonical_token_and_value() {
        // Code Table 4.5 surface type 103 is "specified height above ground".
        let level = grib2_level(103, 2.0).unwrap();
        assert_eq!(level.type_token(), "height_above_ground");
        assert_eq!(level.value(), Some(2.0));
    }

    #[test]
    fn eccodes_height_above_ground_matches_the_grib2_normalization() {
        let level = eccodes_level("heightAboveGround", Some(2));
        assert_eq!(level.type_token(), "height_above_ground");
        assert_eq!(level.value(), Some(2.0));
    }

    #[test]
    fn isobaric_levels_normalize_to_the_same_hectopascal_number_across_backends() {
        // grib-rs reports pascals (surface type 100); ecCodes reports pascals under isobaricInPa and
        // hectopascals under isobaricInhPa. All three must land on the same hPa value.
        let grib_rs = grib2_level(100, 85_000.0).unwrap();
        let from_pascals = eccodes_level("isobaricInPa", Some(85_000));
        let from_hectopascals = eccodes_level("isobaricInhPa", Some(850));
        assert_eq!(grib_rs.type_token(), "isobaric");
        assert_eq!(grib_rs.value(), Some(850.0));
        assert_eq!(from_pascals.value(), Some(850.0));
        assert_eq!(from_hectopascals.value(), Some(850.0));
    }

    #[test]
    fn a_surface_level_type_carries_no_numeric_value() {
        let level = grib2_level(1, f64::NAN).unwrap();
        assert_eq!(level.type_token(), "surface");
        assert_eq!(level.value(), None);
    }

    #[test]
    fn the_missing_surface_type_produces_no_level() {
        assert!(grib2_level(255, f64::NAN).is_none());
    }

    #[test]
    fn an_unknown_grib2_surface_type_keeps_a_stable_fallback_token() {
        let level = grib2_level(240, 5.0).unwrap();
        assert_eq!(level.type_token(), "sfc240");
        assert_eq!(level.value(), Some(5.0));
    }

    #[test]
    fn an_unknown_eccodes_type_of_level_keeps_the_raw_token() {
        let level = eccodes_level("someExoticLevel", Some(3));
        assert_eq!(level.type_token(), "someExoticLevel");
        assert_eq!(level.value(), Some(3.0));
    }
}
