use strum::IntoStaticStr;

/// Code Table 4.5 surface type 255 means "missing": the field carries no vertical level to normalize.
const MISSING_SURFACE: u8 = 255;

/// A Cassiopeia-canonical GRIB vertical level type, normalized across both backends and both editions.
///
/// `snake_case` is forced by the record-consumption path, not a stylistic choice: `level_type` is read
/// bare in a Tera template (`level_type == "height_above_ground"`), so a spaced or kebab token would
/// not compare as a plain identifier. GRIB2 Code Table 4.5 surface types and ecCodes' `typeOfLevel`
/// strings both map onto this shared set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum LevelType {
    /// Ground or water surface.
    Surface,
    /// Mean sea level.
    MeanSeaLevel,
    /// A specified height above the ground (m).
    HeightAboveGround,
    /// A specified altitude above mean sea level (m).
    AltitudeAboveMeanSea,
    /// An isobaric (constant-pressure) surface; canonical unit hPa.
    Isobaric,
    /// A depth below the land surface (m).
    DepthBelowLand,
    /// A sigma (terrain-following) level.
    Sigma,
    /// A model hybrid level.
    Hybrid,
    /// The tropopause.
    Tropopause,
    /// The level of maximum wind.
    MaxWind,
    /// The cloud base level.
    CloudBase,
    /// The cloud top level.
    CloudTop,
    /// The 0 degree Celsius isotherm.
    IsothermZero,
    /// The entire atmosphere considered as a single layer.
    EntireAtmosphere,
    /// The nominal top of the atmosphere.
    TopOfAtmosphere,
    /// A potential vorticity surface.
    PotentialVorticity,
}

impl LevelType {
    /// Recognizes a level type from a GRIB2 Code Table 4.5 surface type. Returns `None` for a surface
    /// type outside the curated set (including 255, "missing").
    const fn from_grib2(surface_type: u8) -> Option<LevelType> {
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

    /// Recognizes a level type from an ecCodes `typeOfLevel` string (both editions). Returns `None` for
    /// a token outside the curated set.
    fn from_eccodes(type_of_level: &str) -> Option<LevelType> {
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

    /// Whether this level type carries a meaningful numeric level value. Surface-like types (surface,
    /// mean sea level, tropopause, cloud base/top, ...) name a single position with no scalar, so the
    /// record omits the numeric `level` for them and keeps only `level_type`.
    const fn has_value(self) -> bool {
        match self {
            LevelType::HeightAboveGround
            | LevelType::AltitudeAboveMeanSea
            | LevelType::Isobaric
            | LevelType::DepthBelowLand
            | LevelType::Sigma
            | LevelType::Hybrid
            | LevelType::PotentialVorticity => true,
            LevelType::Surface
            | LevelType::MeanSeaLevel
            | LevelType::Tropopause
            | LevelType::MaxWind
            | LevelType::CloudBase
            | LevelType::CloudTop
            | LevelType::IsothermZero
            | LevelType::EntireAtmosphere
            | LevelType::TopOfAtmosphere => false,
        }
    }

    /// The canonical `snake_case` token for this level type.
    fn key(self) -> &'static str {
        self.into()
    }
}

/// Returns the value only when it is finite, so a masked or missing surface value becomes an absent
/// numeric level rather than a `NaN` that would poison grouping and record emission.
fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
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
    let value = if matches!(level_type, LevelType::Isobaric) && type_of_level == "isobaricInPa" {
        value / 100.0
    } else {
        value
    };
    finite(value)
}

/// A normalized GRIB vertical level: a bare-templatable type token and, when the type has one, a
/// numeric value in that type's canonical unit.
///
/// The canonical unit is fixed per type so the numeric `level` matches across backends: isobaric
/// levels are hPa (grib-rs and ecCodes both report pascals in some paths, converted here), heights and
/// depths are metres. A level type outside the curated set keeps a stable fallback token (grib-rs
/// `sfc{surface_type}`, ecCodes the raw `typeOfLevel` string), so distinct levels stay distinct for
/// grouping; those rare cases may differ across backends.
#[derive(Debug, Clone)]
pub struct GribLevel {
    type_token: String,
    value: Option<f64>,
}

impl GribLevel {
    /// Assembles a level from an already-resolved token and value.
    const fn new(type_token: String, value: Option<f64>) -> GribLevel {
        GribLevel { type_token, value }
    }

    /// Normalizes a grib-rs GRIB2 level from a Code Table 4.5 surface type and its decoded value.
    ///
    /// Returns `None` for surface type 255 ("missing"), which carries no level. grib-rs reports an isobaric
    /// surface in pascals, converted to the canonical hPa here.
    #[must_use]
    pub fn from_grib2(surface_type: u8, value: f64) -> Option<GribLevel> {
        if surface_type == MISSING_SURFACE {
            return None;
        }
        match LevelType::from_grib2(surface_type) {
            Some(level_type) if level_type.has_value() => {
                let value = if matches!(level_type, LevelType::Isobaric) { value / 100.0 } else { value };
                Some(GribLevel::new(level_type.key().to_owned(), finite(value)))
            }
            Some(level_type) => Some(GribLevel::new(level_type.key().to_owned(), None)),
            None => Some(GribLevel::new(format!("sfc{surface_type}"), finite(value))),
        }
    }

    /// Normalizes an ecCodes level (both editions) from a `typeOfLevel` string and integer level value.
    ///
    /// ecCodes reports the value in the unit named by the `typeOfLevel` token, so `isobaricInPa` is
    /// converted to the canonical hPa while `isobaricInhPa` is already there; heights and depths are
    /// metres as reported.
    #[must_use]
    pub fn from_eccodes(type_of_level: &str, level: Option<i64>) -> GribLevel {
        match LevelType::from_eccodes(type_of_level) {
            Some(level_type) if level_type.has_value() => {
                let value = level.and_then(|raw| eccodes_value(level_type, type_of_level, raw));
                GribLevel::new(level_type.key().to_owned(), value)
            }
            Some(level_type) => GribLevel::new(level_type.key().to_owned(), None),
            None => GribLevel::new(type_of_level.to_owned(), level.and_then(widen)),
        }
    }

    /// The bare-templatable level type token, emitted as the record's `level_type`.
    #[must_use]
    pub fn type_token(&self) -> &str {
        &self.type_token
    }

    /// The numeric level value in the type's canonical unit, when the type carries one.
    #[must_use]
    pub const fn value(&self) -> Option<f64> {
        self.value
    }

    /// A hashable projection for grouping: the value's raw bits keep grouping float-safe, so two fields
    /// at the same level land in the same slice without an `f64` in the key.
    #[must_use]
    pub fn group_key(&self) -> LevelGroupKey {
        LevelGroupKey {
            type_token: self.type_token.clone(),
            value_bits: self.value.map(f64::to_bits),
        }
    }
}

/// The float-safe grouping identity of a [`GribLevel`], used inside a grid-slice key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LevelGroupKey {
    type_token: String,
    value_bits: Option<u64>,
}

#[cfg(test)]
mod tests {
    use crate::grib::level::{GribLevel, LevelType};

    #[test]
    fn a_height_surface_type_normalizes_to_the_canonical_token_and_value() {
        // Code Table 4.5 surface type 103 is "specified height above ground".
        let level = GribLevel::from_grib2(103, 2.0).unwrap();
        assert_eq!(level.type_token(), "height_above_ground");
        assert_eq!(level.value(), Some(2.0));
    }

    #[test]
    fn eccodes_height_above_ground_matches_the_grib2_normalization() {
        let level = GribLevel::from_eccodes("heightAboveGround", Some(2));
        assert_eq!(level.type_token(), "height_above_ground");
        assert_eq!(level.value(), Some(2.0));
    }

    #[test]
    fn isobaric_levels_normalize_to_the_same_hectopascal_number_across_backends() {
        // grib-rs reports pascals (surface type 100); ecCodes reports pascals under isobaricInPa and
        // hectopascals under isobaricInhPa. All three must land on the same hPa value.
        let grib_rs = GribLevel::from_grib2(100, 85_000.0).unwrap();
        let from_pascals = GribLevel::from_eccodes("isobaricInPa", Some(85_000));
        let from_hectopascals = GribLevel::from_eccodes("isobaricInhPa", Some(850));
        assert_eq!(grib_rs.type_token(), "isobaric");
        assert_eq!(grib_rs.value(), Some(850.0));
        assert_eq!(from_pascals.value(), Some(850.0));
        assert_eq!(from_hectopascals.value(), Some(850.0));
    }

    #[test]
    fn a_surface_level_type_carries_no_numeric_value() {
        let level = GribLevel::from_grib2(1, f64::NAN).unwrap();
        assert_eq!(level.type_token(), "surface");
        assert_eq!(level.value(), None);
    }

    #[test]
    fn the_missing_surface_type_produces_no_level() {
        assert!(GribLevel::from_grib2(255, f64::NAN).is_none());
    }

    #[test]
    fn an_unknown_grib2_surface_type_keeps_a_stable_fallback_token() {
        let level = GribLevel::from_grib2(240, 5.0).unwrap();
        assert_eq!(level.type_token(), "sfc240");
        assert_eq!(level.value(), Some(5.0));
    }

    #[test]
    fn an_unknown_eccodes_type_of_level_keeps_the_raw_token() {
        let level = GribLevel::from_eccodes("someExoticLevel", Some(3));
        assert_eq!(level.type_token(), "someExoticLevel");
        assert_eq!(level.value(), Some(3.0));
    }

    #[test]
    fn the_level_type_serializes_to_snake_case() {
        assert_eq!(LevelType::HeightAboveGround.key(), "height_above_ground");
        assert_eq!(LevelType::MeanSeaLevel.key(), "mean_sea_level");
        assert_eq!(LevelType::DepthBelowLand.key(), "depth_below_land");
    }
}
