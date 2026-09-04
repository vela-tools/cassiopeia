use crate::grib::{level::GribLevel, parameter::ParameterName};

/// One decoded GRIB field: a single 2D parameter grid with its coordinates, vertical level, and times.
///
/// This is the edition-agnostic seam between decoding and the pivot. Both decoders (grib-rs for
/// GRIB2, ecCodes for GRIB1) produce this same shape, and everything downstream (the grid-slice
/// pivot, record emission) is identical for both editions.
///
/// Coordinates and values are `f64`: ecCodes decodes GRIB1 in double precision, and GRIB2's single
/// precision widens losslessly into it, so no narrowing cast is needed on either path.
pub struct DecodedField {
    /// A stable per-grid identity: the raw Grid Definition Section bytes for GRIB2, a grid-section
    /// hash for GRIB1. Byte-equal identities share geometry and cell ordering, so values line up cell
    /// for cell across fields.
    pub grid: Vec<u8>,
    /// The parameter this field measures, used as the record column key.
    pub parameter: ParameterName,
    /// Per-cell `(latitude, longitude)` in the grid's scanning order, aligned index-for-index with
    /// `values`. Longitude is always in `[-180, 180)`, west negative; see [`normalize_longitude`].
    pub latlons: Vec<(f64, f64)>,
    /// Per-cell values in the same order as `latlons`; bitmap-masked cells are `NaN`.
    pub values: Vec<f64>,
    /// The normalized vertical level, when the field carries one.
    pub level: Option<GribLevel>,
    /// RFC 3339 reference (analysis) time, when present.
    pub reference_time: Option<String>,
    /// RFC 3339 forecast valid time, when present.
    pub forecast_time: Option<String>,
}

/// Folds a longitude into the `[-180, 180)` range that [`DecodedField::latlons`] guarantees.
///
/// A GRIB grid definition may express longitude in either `[-180, 180)` or `[0, 360)`, and both are
/// valid: the GFS global grid uses the latter, regional grids typically the former. grib-rs folds its
/// own output, so without this the two decoders would disagree on every cell east of the antimeridian
/// and break the parity `DecodedField` exists to provide.
///
/// `180` folds to `-180`, which keeps the antimeridian on one representation and matches grib-rs. A
/// longitude already in range is returned unchanged rather than round-tripped through the arithmetic,
/// so exact grid coordinates stay bit-identical.
#[must_use]
pub fn normalize_longitude(longitude: f64) -> f64 {
    if (-180.0..180.0).contains(&longitude) {
        longitude
    } else {
        (longitude + 180.0).rem_euclid(360.0) - 180.0
    }
}

#[cfg(test)]
mod tests {
    use crate::grib::field::normalize_longitude;

    /// Bit equality, because these are exact grid coordinates rather than computed measurements: a
    /// folded longitude that is merely close would shift a cell and change its geohash identity.
    fn assert_exact(actual: f64, expected: f64) {
        assert_eq!(actual.to_bits(), expected.to_bits(), "expected {expected}, got {actual}");
    }

    #[test]
    fn a_longitude_already_in_range_is_returned_untouched() {
        for longitude in [-180.0, -179.5, -1.0, 0.0, 14.0, 179.999] {
            assert_exact(normalize_longitude(longitude), longitude);
        }
    }

    #[test]
    fn the_zero_to_three_sixty_convention_folds_onto_the_signed_range() {
        assert_exact(normalize_longitude(181.0), -179.0);
        assert_exact(normalize_longitude(270.0), -90.0);
        assert_exact(normalize_longitude(359.0), -1.0);
    }

    #[test]
    fn the_antimeridian_folds_to_the_negative_representation() {
        // grib-rs emits -180 and never 180, so ecCodes must agree for the two grids to line up.
        assert_exact(normalize_longitude(180.0), -180.0);
    }

    #[test]
    fn a_full_turn_returns_to_the_prime_meridian() {
        // 360 degrees is the same meridian as 0, not the antimeridian.
        assert_exact(normalize_longitude(360.0), 0.0);
    }

    #[test]
    fn every_cell_of_a_zero_to_three_sixty_grid_lands_in_the_geojson_range() {
        // RFC 7946 constrains longitude to [-180, 180]; a GFS one-degree row spans 0..359.
        for step in 0..360 {
            let folded = normalize_longitude(f64::from(step));
            assert!((-180.0..180.0).contains(&folded), "{step} folded to {folded}");
        }
    }
}
