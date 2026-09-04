use crate::{geometry::GeometryKind, strategy::ConversionStrategy};
use thiserror::Error;

/// A refusal raised while admitting, building, converting, or validating a geometry.
///
/// Every payload is a small, record-independent value on purpose: a refusal doubles as the
/// deduplication key the extraction stage's diagnostic groups by, so two records refused for the
/// same reason must produce two equal errors.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
pub enum GeometryError {
    /// The source carried a `GeometryCollection`, which is not one of the six geometry types a
    /// `GeoProperty` admits (ETSI GS CIM 009 v1.9.1, clause 4.7).
    #[error(
        "a GeometryCollection is not an admissible GeoProperty value (ETSI GS CIM 009 v1.9.1 clause 4.7); declare `geometry: {{ convert: \"flatten\" }}` to fold it into one geometry"
    )]
    GeometryCollection,

    /// The source and target geometries differ in dimension, and no conversion was declared.
    #[error("a {origin} cannot become a {target} without a declared `geometry: {{ convert: ... }}` conversion")]
    Uncoercible {
        /// The geometry type the source carried.
        origin: GeometryKind,
        /// The geometry type the mapping asked for.
        target: GeometryKind,
    },

    /// A multi-geometry carrying several members was asked to become a single geometry, which
    /// discards every member but one, and no conversion was declared.
    #[error("a {origin} carrying {members} members cannot become a single geometry without a declared `geometry: {{ convert: ... }}` conversion")]
    AmbiguousMultiGeometry {
        /// The multi-geometry type the source carried.
        origin: GeometryKind,
        /// How many members it carried.
        members: usize,
    },

    /// The declared conversion cannot produce the declared target type.
    #[error("the `{strategy}` conversion cannot produce a {target}")]
    StrategyNotApplicable {
        /// The conversion the mapping declared.
        strategy: ConversionStrategy,
        /// The geometry type the mapping asked for.
        target: GeometryKind,
    },

    /// The source coordinates are not nested the way the target geometry type requires.
    #[error("the source coordinates are not shaped like a {target}")]
    Unbuildable {
        /// The geometry type the mapping asked for.
        target: GeometryKind,
    },

    /// A position carried fewer than the two components RFC 7946 clause 3.1.1 requires.
    #[error("a position needs at least a longitude and a latitude, got {components} component(s) (RFC 7946 clause 3.1.1)")]
    ShortPosition {
        /// How many components the position carried.
        components: usize,
    },

    /// A `LineString` carried fewer than the two positions RFC 7946 clause 3.1.4 requires.
    #[error("a LineString needs at least two positions, got {positions} (RFC 7946 clause 3.1.4)")]
    ShortLineString {
        /// How many positions the line carried.
        positions: usize,
    },

    /// A linear ring carried fewer than the four positions RFC 7946 clause 3.1.6 requires.
    #[error("a linear ring needs at least four positions, got {positions} (RFC 7946 clause 3.1.6)")]
    ShortRing {
        /// How many positions the ring carried.
        positions: usize,
    },

    /// A linear ring did not end where it started, and a ring is never closed on the source's
    /// behalf.
    #[error("a linear ring must end at its first position (RFC 7946 clause 3.1.6)")]
    UnclosedRing,

    /// A multi-geometry carrying no members was asked to become a single geometry, which has no
    /// member to fabricate.
    #[error("an empty geometry carries no coordinates to convert")]
    EmptyGeometry,

    /// A `GeometryCollection` whose members are of several geometry families cannot fold into one
    /// geometry.
    #[error("a GeometryCollection of mixed geometry types cannot be folded into one geometry")]
    MixedCollection,
}
