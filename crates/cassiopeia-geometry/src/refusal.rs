use crate::{error::GeometryError, wgs84::OutOfRangePosition};
use thiserror::Error;

/// Why a source geometry could not be typed as the geometry its declaration names.
///
/// The two are kept apart because they are reported apart: a [`GeometryError`] is a
/// record-independent reason that two refused records share, while an out-of-range refusal quotes
/// the position it was met with, which differs from record to record.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum GeometryRefusal {
    /// The geometry breaks a structural rule, is of a type the declaration cannot hold, or needs a
    /// conversion the mapping did not authorise.
    #[error(transparent)]
    Inadmissible(#[from] GeometryError),
    /// A position lies outside WGS 84, which RFC 7946 section 4 fixes every `GeoJSON` coordinate to.
    #[error(transparent)]
    OutsideWgs84(#[from] OutOfRangePosition),
}

#[cfg(test)]
mod tests {
    use crate::{
        error::GeometryError,
        refusal::GeometryRefusal,
        wgs84::{Axis, OutOfRangePosition, PositionPath},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_geometry_error_converts_into_an_inadmissible_refusal_with_its_message() {
        let refusal = GeometryRefusal::from(GeometryError::UnclosedRing);

        assert_eq!(refusal, GeometryRefusal::Inadmissible(GeometryError::UnclosedRing));
        assert_eq!(refusal.to_string(), GeometryError::UnclosedRing.to_string());
    }

    #[test]
    fn an_out_of_range_position_converts_into_a_refusal_with_its_message() {
        let position = OutOfRangePosition {
            axis: Axis::Longitude,
            path: PositionPath::default(),
            position: vec![200.0, 0.0].into(),
        };
        let refusal = GeometryRefusal::from(position.clone());

        assert_eq!(refusal.to_string(), position.to_string());
        assert_eq!(refusal, GeometryRefusal::OutsideWgs84(position));
    }
}
