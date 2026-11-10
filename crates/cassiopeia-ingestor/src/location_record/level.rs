use strum::IntoStaticStr;

/// A Cassiopeia-canonical vertical level type, shared by every gridded and station source format.
///
/// `snake_case` is forced by the record-consumption path, not a stylistic choice: `level_type` is read
/// bare in a Tera template (`level_type == "height_above_ground"`), so a spaced or kebab token would
/// not compare as a plain identifier. GRIB2 Code Table 4.5 surface types, ecCodes' `typeOfLevel`
/// strings, and CF vertical coordinates all map onto this shared set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
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
    /// A depth below the land surface (m), as GRIB2 Code Table 4.5 type 106 and CF soil depth
    /// coordinates express it.
    DepthBelowLand,
    /// A depth below the sea surface (m), as CF ocean depth coordinates express it.
    DepthBelowSea,
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
    /// Whether this level type carries a meaningful numeric level value. Surface-like types (surface,
    /// mean sea level, tropopause, cloud base/top, ...) name a single position with no scalar, so the
    /// record omits the numeric `level` for them and keeps only `level_type`.
    #[must_use]
    pub const fn has_value(self) -> bool {
        match self {
            LevelType::HeightAboveGround
            | LevelType::AltitudeAboveMeanSea
            | LevelType::Isobaric
            | LevelType::DepthBelowLand
            | LevelType::DepthBelowSea
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

/// What kind of level a [`Level`] is: one of the canonical types, or a source token outside them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum LevelKind {
    /// A level type in the canonical set.
    Known(LevelType),
    /// A source-format level the canonical set does not cover, kept under a stable fallback token so
    /// distinct levels stay distinct for grouping.
    Unrecognized(String),
}

/// A normalized vertical level: a bare-templatable type token and, when the type has one, a numeric
/// value in that type's canonical unit.
///
/// The canonical unit is fixed per type so the numeric `level` matches across decoders and formats:
/// isobaric levels are hPa, heights and depths are metres. A level type outside the curated set keeps
/// a stable fallback token, so distinct levels stay distinct for grouping.
#[derive(Debug, Clone)]
pub struct Level {
    kind: LevelKind,
    value: Option<f64>,
}

impl Level {
    /// A level of a canonical type.
    ///
    /// The value is dropped when the type carries none (a surface-like type) or when it is not finite,
    /// so a masked or missing value becomes an absent numeric level rather than a `NaN` that would
    /// poison grouping and record emission.
    #[must_use]
    pub fn new(level_type: LevelType, value: Option<f64>) -> Level {
        Level {
            kind: LevelKind::Known(level_type),
            value: value.filter(|value| level_type.has_value() && value.is_finite()),
        }
    }

    /// A level whose source type is outside the canonical set, kept under the fallback `type_token`.
    ///
    /// A non-finite value is dropped, as for [`Level::new`].
    #[must_use]
    pub fn unrecognized(type_token: String, value: Option<f64>) -> Level {
        Level {
            kind: LevelKind::Unrecognized(type_token),
            value: value.filter(|value| value.is_finite()),
        }
    }

    /// The bare-templatable level type token, emitted as the record's `level_type`.
    #[must_use]
    pub fn type_token(&self) -> &str {
        match &self.kind {
            LevelKind::Known(level_type) => level_type.key(),
            LevelKind::Unrecognized(token) => token,
        }
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
            // The key outlives the level it groups, so it owns a copy of the (rare) fallback token.
            kind: self.kind.clone(),
            value_bits: self.value.map(f64::to_bits),
        }
    }
}

/// The float-safe grouping identity of a [`Level`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LevelGroupKey {
    kind: LevelKind,
    value_bits: Option<u64>,
}

#[cfg(test)]
mod tests {
    use crate::location_record::level::{Level, LevelType};
    use pretty_assertions::{assert_eq, assert_ne};

    #[test]
    fn an_isobaric_level_carries_its_token_and_hectopascal_value() {
        let level = Level::new(LevelType::Isobaric, Some(850.0));
        assert_eq!(level.type_token(), "isobaric");
        assert_eq!(level.value(), Some(850.0));
    }

    #[test]
    fn a_surface_level_drops_the_value_its_type_does_not_carry() {
        let level = Level::new(LevelType::Surface, Some(0.0));
        assert_eq!(level.type_token(), "surface");
        assert_eq!(level.value(), None);
    }

    #[test]
    fn a_non_finite_value_becomes_an_absent_level_value() {
        assert_eq!(Level::new(LevelType::HeightAboveGround, Some(f64::NAN)).value(), None);
        assert_eq!(Level::unrecognized("sfc240".to_string(), Some(f64::INFINITY)).value(), None);
    }

    #[test]
    fn depth_below_sea_serializes_to_snake_case_and_carries_a_value() {
        let level = Level::new(LevelType::DepthBelowSea, Some(0.494));
        assert_eq!(level.type_token(), "depth_below_sea");
        assert_eq!(level.value(), Some(0.494));
    }

    #[test]
    fn an_unrecognized_level_keeps_its_fallback_token() {
        let level = Level::unrecognized("someExoticLevel".to_string(), Some(3.0));
        assert_eq!(level.type_token(), "someExoticLevel");
        assert_eq!(level.value(), Some(3.0));
    }

    #[test]
    fn the_level_type_serializes_to_snake_case() {
        assert_eq!(LevelType::HeightAboveGround.key(), "height_above_ground");
        assert_eq!(LevelType::MeanSeaLevel.key(), "mean_sea_level");
        assert_eq!(LevelType::DepthBelowLand.key(), "depth_below_land");
    }

    #[test]
    fn the_same_level_groups_together_and_different_values_apart() {
        let first = Level::new(LevelType::Isobaric, Some(850.0));
        let second = Level::new(LevelType::Isobaric, Some(850.0));
        let other = Level::new(LevelType::Isobaric, Some(500.0));
        assert_eq!(first.group_key(), second.group_key());
        assert_ne!(first.group_key(), other.group_key());
    }
}
