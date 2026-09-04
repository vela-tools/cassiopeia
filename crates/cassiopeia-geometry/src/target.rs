use crate::geometry::GeometryKind;

/// What a mapping's `transformation` asks the geometry to become.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GeometryTarget {
    /// Keep whatever admissible geometry the source carries, whichever of the six it is.
    Preserve,
    /// Produce exactly this geometry type.
    Coerce(GeometryKind),
}
