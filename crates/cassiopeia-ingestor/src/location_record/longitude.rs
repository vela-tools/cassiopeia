/// Folds a longitude into the `[-180, 180)` range every location record carries.
///
/// A gridded source may express longitude in either `[-180, 180)` or `[0, 360)`, and both are valid:
/// the GFS global GRIB grid and many CF global `NetCDF` grids use the latter, regional grids typically
/// the former. grib-rs folds its own output, so without this the decoders would disagree on every
/// cell east of the antimeridian, and a `[0, 360)` value would fall outside the RFC 7946 range a
/// `GeoJSON` position allows.
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
    use crate::location_record::longitude::normalize_longitude;
    use pretty_assertions::assert_eq;

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
