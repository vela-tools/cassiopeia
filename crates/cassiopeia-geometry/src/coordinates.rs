use geojson::{LineStringType, PolygonType, Position};

/// Raw coordinates read out of a source value, before they carry a geometry type.
///
/// A source that writes its geometry as a bare coordinate array (a lon/lat pair in two CSV
/// columns, a list of pairs in a JSON field) says nothing about which geometry type it means. The
/// mapping's declared target supplies that, and this carries the nesting the target is matched
/// against.
#[derive(Clone, Debug, PartialEq)]
pub enum CoordinateShape {
    /// One position: `[x, y]`, or `[x, y, z]` carrying an altitude (RFC 7946 clause 3.1.1).
    Position(Position),
    /// An ordered group of shapes, one nesting level above its members.
    Group(Vec<CoordinateShape>),
}

impl CoordinateShape {
    /// How deeply the coordinates nest: a position is zero, a list of positions one, a list of
    /// those two, and so on. An empty group counts as one level with nothing under it.
    #[must_use]
    pub fn depth(&self) -> usize {
        match self {
            CoordinateShape::Position(_) => 0,
            CoordinateShape::Group(members) => 1 + members.iter().map(CoordinateShape::depth).max().unwrap_or(0),
        }
    }

    /// Reads the shape as one position.
    #[must_use]
    pub fn into_position(self) -> Option<Position> {
        match self {
            CoordinateShape::Position(position) => Some(position),
            CoordinateShape::Group(_) => None,
        }
    }

    /// Reads the shape as a list of positions: a curve, or a `MultiPoint`'s members.
    #[must_use]
    pub fn into_positions(self) -> Option<LineStringType> {
        self.into_group()?.into_iter().map(CoordinateShape::into_position).collect()
    }

    /// Reads the shape as a list of position lists: a polygon's rings, or a `MultiLineString`'s
    /// curves.
    #[must_use]
    pub fn into_rings(self) -> Option<PolygonType> {
        self.into_group()?.into_iter().map(CoordinateShape::into_positions).collect()
    }

    /// Reads the shape as a list of ring lists: a `MultiPolygon`'s surfaces.
    #[must_use]
    pub fn into_polygons(self) -> Option<Vec<PolygonType>> {
        self.into_group()?.into_iter().map(CoordinateShape::into_rings).collect()
    }

    /// Reads the shape as a group, rejecting a bare position.
    fn into_group(self) -> Option<Vec<CoordinateShape>> {
        match self {
            CoordinateShape::Position(_) => None,
            CoordinateShape::Group(members) => Some(members),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::coordinates::CoordinateShape;
    use geojson::Position;

    fn position(x: f64, y: f64) -> CoordinateShape {
        CoordinateShape::Position(Position::from([x, y]))
    }

    #[test]
    fn depth_counts_the_nesting_levels() {
        assert_eq!(position(1.0, 2.0).depth(), 0);
        assert_eq!(CoordinateShape::Group(vec![position(1.0, 2.0)]).depth(), 1);
        assert_eq!(CoordinateShape::Group(vec![CoordinateShape::Group(vec![position(1.0, 2.0)])]).depth(), 2);
    }

    #[test]
    fn a_flat_group_reads_as_positions_but_not_as_rings() {
        let shape = CoordinateShape::Group(vec![position(1.0, 2.0), position(3.0, 4.0)]);

        assert_eq!(shape.clone().into_positions().map(|positions| positions.len()), Some(2));
        assert!(shape.into_rings().is_none());
    }

    #[test]
    fn a_bare_position_reads_only_as_a_position() {
        assert!(position(1.0, 2.0).into_position().is_some());
        assert!(position(1.0, 2.0).into_positions().is_none());
    }

    #[test]
    fn a_doubly_nested_group_reads_as_rings() {
        let shape = CoordinateShape::Group(vec![CoordinateShape::Group(vec![position(1.0, 2.0), position(3.0, 4.0)])]);

        assert_eq!(shape.into_rings().map(|rings| rings.len()), Some(1));
    }

    #[test]
    fn a_triply_nested_group_reads_as_polygons() {
        let shape = CoordinateShape::Group(vec![CoordinateShape::Group(vec![CoordinateShape::Group(vec![position(1.0, 2.0)])])]);

        assert_eq!(shape.into_polygons().map(|polygons| polygons.len()), Some(1));
    }
}
