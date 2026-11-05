use crate::geometry::NgsiLdGeometry;
use derive_more::Display;
use geojson::Position;
use std::{fmt, ops::RangeInclusive};
use thiserror::Error;

/// The longitudes WGS 84 spans, in decimal degrees (RFC 7946 section 4).
const LONGITUDES: RangeInclusive<f64> = -180.0..=180.0;

/// The latitudes WGS 84 spans, in decimal degrees (RFC 7946 section 4).
const LATITUDES: RangeInclusive<f64> = -90.0..=90.0;

/// One of the two components of a position that WGS 84 bounds.
///
/// A position's optional third component, its altitude (RFC 7946 clause 3.1.1), has no range to
/// leave, so it is not one of them.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
pub enum Axis {
    /// The first component, bounded to [-180, 180].
    #[display("longitude")]
    Longitude,
    /// The second component, bounded to [-90, 90].
    #[display("latitude")]
    Latitude,
}

/// Where a position sits in its geometry: the index taken at each level of the `coordinates`
/// nesting, outermost first.
///
/// A `Point`'s one position sits at no index, so its path is empty and reads as `coordinates`.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct PositionPath(Vec<usize>);

impl PositionPath {
    /// The path of the same position seen from one level further out, where its enclosing member
    /// sits at `index`.
    fn under(mut self, index: usize) -> PositionPath {
        self.0.insert(0, index);
        self
    }
}

// Manual because the path renders as the indexing expression that reaches the position, which no
// derive expresses.
impl fmt::Display for PositionPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("coordinates")?;
        self.0.iter().try_for_each(|index| write!(f, "[{index}]"))
    }
}

/// A position whose longitude or latitude is outside WGS 84.
///
/// RFC 7946 section 4 fixes every `GeoJSON` coordinate to WGS 84 longitude and latitude in decimal
/// degrees, so a component outside the range is not a mistake of shape but a sign that the numbers
/// mean something else: coordinates in a projected reference system the source did not declare,
/// longitude and latitude swapped, or a wrong unit or scale. The position is the first one found,
/// kept with where it sits so the source can be looked up.
#[derive(Clone, Debug, Error, PartialEq)]
#[error("the position `[{}]` at `{path}` has a {axis} outside WGS 84", render(position))]
pub struct OutOfRangePosition {
    /// The component that left its range; the longitude when both did.
    pub axis: Axis,
    /// Where the position sits in its geometry.
    pub path: PositionPath,
    /// The position as the geometry carries it.
    pub position: Position,
}

impl OutOfRangePosition {
    /// The same refusal seen from one level further out, where the member holding the position
    /// sits at `index`.
    fn under(self, index: usize) -> OutOfRangePosition {
        OutOfRangePosition {
            path: self.path.under(index),
            ..self
        }
    }
}

/// Checks that every position of `geometry` lies within WGS 84.
///
/// A position too short to carry a latitude is left to the structural check, which names it for
/// what it is.
///
/// # Errors
/// Returns the first [`OutOfRangePosition`], in the order the geometry lists its positions.
pub fn check_range(geometry: &NgsiLdGeometry) -> Result<(), OutOfRangePosition> {
    let found = match geometry {
        NgsiLdGeometry::Point { coordinates } => coordinates.first_outside(),
        NgsiLdGeometry::MultiPoint { coordinates } | NgsiLdGeometry::LineString { coordinates } => coordinates.first_outside(),
        NgsiLdGeometry::MultiLineString { coordinates } | NgsiLdGeometry::Polygon { coordinates } => coordinates.first_outside(),
        NgsiLdGeometry::MultiPolygon { coordinates } => coordinates.first_outside(),
    };

    found.map_or(Ok(()), Err)
}

/// A level of a geometry's `coordinates` nesting: one position, or an array of deeper levels.
trait Positions {
    /// The first position at or under this level outside WGS 84, its path taken from this level.
    fn first_outside(&self) -> Option<OutOfRangePosition>;
}

impl Positions for Position {
    fn first_outside(&self) -> Option<OutOfRangePosition> {
        let outside = |index: usize, range: &RangeInclusive<f64>| self.as_slice().get(index).is_some_and(|component| !range.contains(component));
        let axis = if outside(0, &LONGITUDES) {
            Axis::Longitude
        } else if outside(1, &LATITUDES) {
            Axis::Latitude
        } else {
            return None;
        };

        Some(OutOfRangePosition {
            axis,
            path: PositionPath::default(),
            // The refusal is built once per refused geometry and quotes the position back, so it
            // keeps its own copy of the few components it names.
            position: self.clone(),
        })
    }
}

impl<T: Positions> Positions for Vec<T> {
    fn first_outside(&self) -> Option<OutOfRangePosition> {
        self.iter()
            .enumerate()
            .find_map(|(index, member)| member.first_outside().map(|found| found.under(index)))
    }
}

/// Renders a position's components as the `GeoJSON` array that spells them, without its brackets.
fn render(position: &Position) -> String {
    position.as_slice().iter().map(f64::to_string).collect::<Vec<String>>().join(", ")
}

#[cfg(test)]
mod tests {
    use crate::{
        geometry::NgsiLdGeometry,
        wgs84::{Axis, OutOfRangePosition, PositionPath, check_range},
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn geometry(document: serde_json::Value) -> NgsiLdGeometry {
        serde_json::from_value(document).expect("admissible geometry")
    }

    fn refusal(axis: Axis, path: Vec<usize>, position: Vec<f64>) -> OutOfRangePosition {
        OutOfRangePosition {
            axis,
            path: PositionPath(path),
            position: position.into(),
        }
    }

    #[test]
    fn a_geometry_within_wgs_84_passes_up_to_and_including_the_bounds() {
        for document in [
            json!({"type": "Point", "coordinates": [14.5, 46.05]}),
            json!({"type": "Point", "coordinates": [-180.0, -90.0]}),
            json!({"type": "Point", "coordinates": [180.0, 90.0]}),
            json!({"type": "LineString", "coordinates": [[0.0, 0.0], [179.9, 89.9]]}),
            json!({"type": "MultiPolygon", "coordinates": [[[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]]}),
        ] {
            assert_eq!(check_range(&geometry(document.clone())), Ok(()), "{document}");
        }
    }

    #[test]
    fn an_altitude_of_any_size_is_not_range_checked() {
        let document = json!({"type": "Point", "coordinates": [14.5, 46.05, 341_870.2]});

        assert_eq!(check_range(&geometry(document)), Ok(()));
    }

    #[test]
    fn a_point_outside_wgs_84_is_refused_at_its_coordinates() {
        assert_eq!(
            check_range(&geometry(json!({"type": "Point", "coordinates": [2600.5, 341_870.2]}))),
            Err(refusal(Axis::Longitude, Vec::new(), vec![2600.5, 341_870.2]))
        );
        assert_eq!(
            check_range(&geometry(json!({"type": "Point", "coordinates": [16.37, 91.0]}))),
            Err(refusal(Axis::Latitude, Vec::new(), vec![16.37, 91.0]))
        );
    }

    #[test]
    fn the_first_position_outside_wgs_84_is_named_by_its_path_at_every_nesting_depth() {
        for (document, path, position) in [
            (
                json!({"type": "MultiPoint", "coordinates": [[1.0, 2.0], [-180.5, 2.0], [500.0, 2.0]]}),
                vec![1],
                vec![-180.5, 2.0],
            ),
            (
                json!({"type": "MultiLineString", "coordinates": [[[1.0, 2.0], [3.0, 4.0]], [[5.0, 6.0], [7.0, 8.0], [181.0, 8.0]]]}),
                vec![1, 2],
                vec![181.0, 8.0],
            ),
            (
                json!({"type": "MultiPolygon", "coordinates": [
                    [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                    [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]], [[0.0, 0.0], [200.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                ]}),
                vec![1, 1, 1],
                vec![200.0, 0.0],
            ),
        ] {
            assert_eq!(
                check_range(&geometry(document.clone())),
                Err(refusal(Axis::Longitude, path, position)),
                "{document}"
            );
        }
    }

    #[test]
    fn a_position_with_both_components_outside_names_its_longitude() {
        let document = json!({"type": "Point", "coordinates": [2600.5, 341_870.2]});

        assert_eq!(check_range(&geometry(document)).map_err(|refused| refused.axis), Err(Axis::Longitude));
    }

    #[test]
    fn the_refusal_quotes_the_position_its_path_and_the_axis_that_left_its_range() {
        let message = refusal(Axis::Latitude, vec![0, 2], vec![16.37, 91.0, 300.0]).to_string();

        assert_eq!(message, "the position `[16.37, 91, 300]` at `coordinates[0][2]` has a latitude outside WGS 84");
    }

    #[test]
    fn a_point_s_path_reads_as_its_coordinates() {
        assert_eq!(PositionPath::default().to_string(), "coordinates");
    }
}
