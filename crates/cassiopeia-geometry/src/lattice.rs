use crate::{
    error::GeometryError,
    geometry::{Cardinality, GeometryKind, NgsiLdGeometry},
    strategy::ConversionStrategy,
    target::GeometryTarget,
};

/// Checks that a declared conversion can produce a declared target type.
///
/// This runs when a mapping is loaded, so a target and a conversion that can never agree (asking
/// `largest` for a `point`, say, when there is no ordering on positions to take a largest of) stops
/// the run before a single record is read rather than refusing every record in turn.
///
/// # Errors
/// Returns [`GeometryError::StrategyNotApplicable`] when the conversion cannot produce the target's
/// dimension.
pub const fn check(target: GeometryTarget, strategy: Option<ConversionStrategy>) -> Result<(), GeometryError> {
    // A preserved target takes whatever the conversion yields, and an absent conversion is the
    // lossless default, so only a declared pair of both can disagree.
    let (GeometryTarget::Coerce(kind), Some(strategy)) = (target, strategy) else {
        return Ok(());
    };

    if strategy.admits(kind.dimension()) {
        Ok(())
    } else {
        Err(GeometryError::StrategyNotApplicable { strategy, target: kind })
    }
}

/// Reconciles a geometry with the target type using only the conversions that lose nothing.
///
/// Three classes pass here: identity, promotion of a single geometry to the multi-geometry of the
/// same dimension, and unwrapping a multi-geometry that carries exactly one member. Everything else
/// discards coordinates and is refused, naming what the mapping would have to declare.
///
/// # Errors
/// Returns [`GeometryError::Uncoercible`] when the dimensions differ,
/// [`GeometryError::AmbiguousMultiGeometry`] when several members would be thrown away, and
/// [`GeometryError::EmptyGeometry`] when there is no member to unwrap.
pub fn reconcile(geometry: NgsiLdGeometry, target: GeometryKind) -> Result<NgsiLdGeometry, GeometryError> {
    let origin = geometry.kind();
    if origin == target {
        return Ok(geometry);
    }
    if origin.dimension() != target.dimension() {
        return Err(GeometryError::Uncoercible { origin, target });
    }

    match target.cardinality() {
        Cardinality::Multi => Ok(promote(geometry)),
        Cardinality::Single => demote(geometry),
    }
}

/// Wraps a single geometry as the multi-geometry of its own dimension.
///
/// A multi-geometry already of the target's cardinality never reaches this, so the multi arms
/// return the geometry unchanged only to keep the match exhaustive.
fn promote(geometry: NgsiLdGeometry) -> NgsiLdGeometry {
    match geometry {
        NgsiLdGeometry::Point { coordinates } => NgsiLdGeometry::MultiPoint {
            coordinates: vec![coordinates],
        },
        NgsiLdGeometry::LineString { coordinates } => NgsiLdGeometry::MultiLineString {
            coordinates: vec![coordinates],
        },
        NgsiLdGeometry::Polygon { coordinates } => NgsiLdGeometry::MultiPolygon {
            coordinates: vec![coordinates],
        },
        multi @ (NgsiLdGeometry::MultiPoint { .. } | NgsiLdGeometry::MultiLineString { .. } | NgsiLdGeometry::MultiPolygon { .. }) => multi,
    }
}

/// Unwraps a multi-geometry that carries exactly one member.
///
/// A single geometry already of the target's cardinality never reaches this, so the single arms
/// return the geometry unchanged only to keep the match exhaustive.
fn demote(geometry: NgsiLdGeometry) -> Result<NgsiLdGeometry, GeometryError> {
    let origin = geometry.kind();
    let members = geometry.members();
    match geometry {
        NgsiLdGeometry::MultiPoint { mut coordinates } if members == 1 => Ok(NgsiLdGeometry::Point {
            coordinates: coordinates.swap_remove(0),
        }),
        NgsiLdGeometry::MultiLineString { mut coordinates } if members == 1 => Ok(NgsiLdGeometry::LineString {
            coordinates: coordinates.swap_remove(0),
        }),
        NgsiLdGeometry::MultiPolygon { mut coordinates } if members == 1 => Ok(NgsiLdGeometry::Polygon {
            coordinates: coordinates.swap_remove(0),
        }),
        NgsiLdGeometry::MultiPoint { .. } | NgsiLdGeometry::MultiLineString { .. } | NgsiLdGeometry::MultiPolygon { .. } => {
            if members == 0 {
                Err(GeometryError::EmptyGeometry)
            } else {
                Err(GeometryError::AmbiguousMultiGeometry { origin, members })
            }
        }
        single @ (NgsiLdGeometry::Point { .. } | NgsiLdGeometry::LineString { .. } | NgsiLdGeometry::Polygon { .. }) => Ok(single),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::GeometryError,
        geometry::{GeometryKind, NgsiLdGeometry},
        lattice::{check, reconcile},
        strategy::ConversionStrategy,
        target::GeometryTarget,
    };
    use geojson::Position;

    fn point() -> NgsiLdGeometry {
        NgsiLdGeometry::Point {
            coordinates: Position::from([1.0, 2.0]),
        }
    }

    fn multi_point(count: usize) -> NgsiLdGeometry {
        NgsiLdGeometry::MultiPoint {
            coordinates: vec![Position::from([1.0, 2.0]); count],
        }
    }

    #[test]
    fn largest_toward_a_point_target_is_refused_when_the_mapping_loads() {
        assert_eq!(
            check(GeometryTarget::Coerce(GeometryKind::Point), Some(ConversionStrategy::Largest)),
            Err(GeometryError::StrategyNotApplicable {
                strategy: ConversionStrategy::Largest,
                target: GeometryKind::Point,
            })
        );
    }

    #[test]
    fn a_conversion_matching_its_target_dimension_loads() {
        assert_eq!(check(GeometryTarget::Coerce(GeometryKind::Polygon), Some(ConversionStrategy::Largest)), Ok(()));
        assert_eq!(
            check(GeometryTarget::Coerce(GeometryKind::MultiPoint), Some(ConversionStrategy::Centroid)),
            Ok(())
        );
    }

    #[test]
    fn a_preserved_target_or_an_absent_conversion_always_loads() {
        assert_eq!(check(GeometryTarget::Preserve, Some(ConversionStrategy::Largest)), Ok(()));
        assert_eq!(check(GeometryTarget::Coerce(GeometryKind::Point), None), Ok(()));
    }

    #[test]
    fn identity_and_promotion_pass_without_a_declaration() {
        assert_eq!(reconcile(point(), GeometryKind::Point), Ok(point()));
        assert_eq!(reconcile(point(), GeometryKind::MultiPoint), Ok(multi_point(1)));
    }

    #[test]
    fn a_multi_geometry_of_one_member_unwraps_without_a_declaration() {
        assert_eq!(reconcile(multi_point(1), GeometryKind::Point), Ok(point()));
    }

    #[test]
    fn a_multi_geometry_of_several_members_names_what_it_would_throw_away() {
        assert_eq!(
            reconcile(multi_point(3), GeometryKind::Point),
            Err(GeometryError::AmbiguousMultiGeometry {
                origin: GeometryKind::MultiPoint,
                members: 3,
            })
        );
    }

    #[test]
    fn an_empty_multi_geometry_is_refused_rather_than_fabricated() {
        assert_eq!(reconcile(multi_point(0), GeometryKind::Point), Err(GeometryError::EmptyGeometry));
    }

    #[test]
    fn a_change_of_dimension_is_refused_without_a_declaration() {
        assert_eq!(
            reconcile(point(), GeometryKind::Polygon),
            Err(GeometryError::Uncoercible {
                origin: GeometryKind::Point,
                target: GeometryKind::Polygon,
            })
        );
    }
}
