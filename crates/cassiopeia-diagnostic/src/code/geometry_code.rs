use strum::{Display, EnumCount, EnumIter};

/// Why a `GeoProperty`'s geometry was refused rather than emitted.
///
/// One code per refusal in the geometry layer, so a run summary can say which structural rule the
/// source data broke without repeating the full sentence for every affected record.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum GeometryCode {
    /// The source carried a `GeometryCollection`, which ETSI GS CIM 009 v1.9.1 clause 4.7 does not
    /// admit as a `GeoProperty` value.
    CollectionInadmissible,
    /// Source and target geometries differ in dimension and no conversion was declared.
    Uncoercible,
    /// A multi-geometry of several members was asked to become a single geometry.
    AmbiguousMultiGeometry,
    /// The declared conversion cannot produce the declared target type.
    StrategyNotApplicable,
    /// The source coordinates are not nested the way the target type requires.
    Unbuildable,
    /// A position carried fewer than the two components RFC 7946 clause 3.1.1 requires.
    ShortPosition,
    /// A `LineString` carried fewer than the two positions RFC 7946 clause 3.1.4 requires.
    ShortLineString,
    /// A linear ring carried fewer than the four positions RFC 7946 clause 3.1.6 requires.
    ShortRing,
    /// A linear ring did not end where it started.
    UnclosedRing,
    /// An empty geometry carries no coordinates to convert.
    EmptyGeometry,
    /// A `GeometryCollection` of mixed geometry families cannot fold into one geometry.
    MixedCollection,
}

#[cfg(test)]
mod tests {
    use crate::code::geometry_code::GeometryCode;

    #[test]
    fn a_geometry_code_renders_a_kebab_case_token() {
        assert_eq!(GeometryCode::AmbiguousMultiGeometry.to_string(), "ambiguous-multi-geometry");
    }
}
