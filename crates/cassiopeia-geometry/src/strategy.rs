use crate::geometry::Dimension;
use serde::{Deserialize, Serialize};
use strum::Display;

/// A named conversion between geometry types, declared in a mapping's `geometry` block.
///
/// Every conversion here discards something (a member, a hole, an extent, an ordering), which is
/// exactly why it has to be named in the mapping rather than applied on the source's behalf. The
/// lossless conversions (identity, promotion to a multi-geometry, and unwrapping a multi-geometry
/// of exactly one member) need no declaration and are always applied.
#[derive(Clone, Copy, Debug, Deserialize, Display, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ConversionStrategy {
    /// Keep the first member of a multi-geometry, in coordinate-array order.
    First,

    /// Keep the largest member of a multi-geometry: the greatest geodesic area for a surface, the
    /// greatest geodesic length for a curve. There is no ordering on positions, so this cannot
    /// produce a geometry of dimension zero.
    Largest,

    /// Take the geometry's centroid. Planar, so it drifts at high latitude and across the
    /// antimeridian, and it may fall outside a concave surface.
    Centroid,

    /// Take a position guaranteed to lie on or inside the geometry. Planar, with the same caveats
    /// as the centroid, but always inside the surface.
    PointOnSurface,

    /// Take the first position the geometry lists.
    FirstVertex,

    /// Take the centre of the geometry's bounding box.
    BboxCenter,

    /// Take a polygon's exterior ring as a closed curve, discarding its holes.
    ExteriorRing,

    /// Take a polygon's whole boundary as a curve per ring, exterior first: every coordinate
    /// survives, only the surface interpretation is dropped.
    Boundary,

    /// Join a `MultiPoint`'s positions into one curve, in coordinate-array order.
    Connect,

    /// Read a closed curve as a polygon's exterior ring. The curve must already close and hold at
    /// least four positions (RFC 7946 clause 3.1.6); an open curve is refused, never closed on the
    /// source's behalf.
    Ring,

    /// Take the convex hull of the geometry's positions.
    ConvexHull,

    /// Take the geometry's bounding box as a rectangular polygon.
    Envelope,

    /// Take every distinct position the geometry lists, in order, as a `MultiPoint`. A polygon
    /// contributes the positions of all its rings, not only the exterior one.
    Vertices,

    /// Fold a `GeometryCollection` into one admissible geometry: a lone member becomes that
    /// geometry, members of one dimension become the matching multi-geometry, and a mixed
    /// collection is refused (ETSI GS CIM 009 v1.9.1 clause 4.7 with RFC 7946 clause 3.1.8).
    Flatten,
}

impl ConversionStrategy {
    /// Whether the conversion can produce a geometry of the given dimension.
    ///
    /// This is what makes a mapping's `geometry` block checkable before a single record is read: a
    /// conversion that follows the source's own dimension admits every target, a derivation admits
    /// only the dimension it produces, and `largest` orders members by extent, which a position
    /// does not have, so it can never reach dimension zero.
    #[must_use]
    pub const fn admits(self, dimension: Dimension) -> bool {
        match self {
            ConversionStrategy::First | ConversionStrategy::Flatten => true,
            ConversionStrategy::Largest => !matches!(dimension, Dimension::Point),
            ConversionStrategy::Centroid
            | ConversionStrategy::PointOnSurface
            | ConversionStrategy::FirstVertex
            | ConversionStrategy::BboxCenter
            | ConversionStrategy::Vertices => matches!(dimension, Dimension::Point),
            ConversionStrategy::ExteriorRing | ConversionStrategy::Boundary | ConversionStrategy::Connect => matches!(dimension, Dimension::Line),
            ConversionStrategy::Ring | ConversionStrategy::ConvexHull | ConversionStrategy::Envelope => matches!(dimension, Dimension::Area),
        }
    }
}

/// Whether a polygon's rings are rewound to RFC 7946 clause 3.1.6's right-hand rule.
#[derive(Clone, Copy, Debug, Default, Deserialize, Display, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Winding {
    /// Rewind every ring to the right-hand rule: exterior rings counterclockwise, holes clockwise.
    /// Clause 3.1.6 states this as a producer requirement, so it is the default.
    #[default]
    Rfc7946,
    /// Emit every ring exactly as the source wound it.
    Keep,
}

/// Whether a position's optional third element survives.
#[derive(Clone, Copy, Debug, Default, Deserialize, Display, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Altitude {
    /// Keep the altitude RFC 7946 clause 3.1.1 permits wherever the conversion can carry it.
    #[default]
    Keep,
    /// Truncate every position to a longitude and a latitude, whatever the source carried.
    Drop,
}

#[cfg(test)]
mod tests {
    use crate::{
        geometry::Dimension,
        strategy::{Altitude, ConversionStrategy, Winding},
    };

    #[test]
    fn strategies_read_and_write_their_kebab_case_tokens() {
        assert_eq!(
            serde_json::from_str::<ConversionStrategy>(r#""point-on-surface""#).unwrap(),
            ConversionStrategy::PointOnSurface
        );
        assert_eq!(serde_json::to_string(&ConversionStrategy::ConvexHull).unwrap(), r#""convex-hull""#);
        assert_eq!(ConversionStrategy::BboxCenter.to_string(), "bbox-center");
    }

    #[test]
    fn an_unrecognised_strategy_is_rejected() {
        assert!(serde_json::from_str::<ConversionStrategy>(r#""buffer""#).is_err());
    }

    #[test]
    fn the_defaults_normalise_winding_and_keep_altitude() {
        assert_eq!(Winding::default(), Winding::Rfc7946);
        assert_eq!(Altitude::default(), Altitude::Keep);
        assert_eq!(serde_json::from_str::<Winding>(r#""rfc7946""#).unwrap(), Winding::Rfc7946);
        assert_eq!(serde_json::from_str::<Altitude>(r#""drop""#).unwrap(), Altitude::Drop);
    }

    #[test]
    fn a_strategy_that_follows_the_source_admits_every_target_dimension() {
        for dimension in [Dimension::Point, Dimension::Line, Dimension::Area] {
            assert!(ConversionStrategy::First.admits(dimension));
            assert!(ConversionStrategy::Flatten.admits(dimension));
        }
    }

    #[test]
    fn largest_admits_every_dimension_that_has_an_extent_to_order_by() {
        assert!(!ConversionStrategy::Largest.admits(Dimension::Point));
        assert!(ConversionStrategy::Largest.admits(Dimension::Line));
        assert!(ConversionStrategy::Largest.admits(Dimension::Area));
    }

    #[test]
    fn every_derivation_admits_only_the_dimension_it_produces() {
        assert!(ConversionStrategy::Centroid.admits(Dimension::Point));
        assert!(!ConversionStrategy::Centroid.admits(Dimension::Area));
        assert!(ConversionStrategy::Boundary.admits(Dimension::Line));
        assert!(!ConversionStrategy::Boundary.admits(Dimension::Point));
        assert!(ConversionStrategy::Envelope.admits(Dimension::Area));
        assert!(!ConversionStrategy::Envelope.admits(Dimension::Line));
    }
}
