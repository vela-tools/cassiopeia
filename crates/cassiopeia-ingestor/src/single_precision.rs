use zmij::Buffer;

/// The `f64` a single-precision source value reaches a record as: the double nearest the shortest
/// decimal that reads back as `value`.
///
/// Widening a `float` exactly carries the binary expansion of the single-precision number into the
/// record, so a stored `34.408` would be written as `34.40800094604492`. Its shortest decimal is the
/// number the source meant and the one netCDF4-python and xarray show, and narrowed back to single
/// precision it is `value` again, so nothing is lost. A fixed count of significant digits would not
/// be safe: some single-precision numbers need nine to read back as themselves.
///
/// `NaN` and the infinities pass through unchanged.
#[must_use]
pub fn shortest_decimal(value: f32) -> f64 {
    let mut buffer = Buffer::new();
    // zmij writes every number, `NaN` and the infinities included, in a form Rust's float parser
    // reads, so the parse does not fail; were it ever to, the exact widening stands for the same
    // single-precision number.
    buffer.format(value).parse().unwrap_or(f64::from(value))
}

#[cfg(test)]
mod tests {
    use crate::single_precision::shortest_decimal;
    use num_traits::AsPrimitive;
    use pretty_assertions::assert_eq;

    /// The bits of `value`, so values compare exactly, `-0.0` apart from `0.0`.
    fn bits(value: f64) -> u64 {
        value.to_bits()
    }

    #[test]
    fn a_single_precision_value_is_written_as_its_shortest_decimal() {
        assert_eq!(bits(shortest_decimal(34.408)), bits(34.408));
        assert_eq!(bits(shortest_decimal(-1.67)), bits(-1.67));
        assert_eq!(bits(shortest_decimal(0.1)), bits(0.1));
    }

    #[test]
    fn a_whole_number_keeps_its_value() {
        assert_eq!(bits(shortest_decimal(280.0)), bits(280.0));
        assert_eq!(bits(shortest_decimal(16_777_216.0)), bits(16_777_216.0));
    }

    #[test]
    fn the_extremes_of_single_precision_are_written_as_their_shortest_decimals() {
        assert_eq!(bits(shortest_decimal(f32::MAX)), bits(3.402_823_5e38));
        assert_eq!(bits(shortest_decimal(f32::MIN_POSITIVE)), bits(1.175_494_4e-38));
        assert_eq!(bits(shortest_decimal(f32::from_bits(1))), bits(1e-45));
    }

    #[test]
    fn every_sampled_single_precision_value_reads_back_as_itself() {
        // Every 4099th bit pattern spans each exponent, subnormals included, with varied mantissas.
        let finite = (0..=u32::MAX).step_by(4099).map(f32::from_bits).filter(|value| value.is_finite());
        for value in finite {
            let narrowed: f32 = shortest_decimal(value).as_();
            assert_eq!(narrowed.to_bits(), value.to_bits(), "{value:e} did not read back as itself");
        }
    }

    #[test]
    fn negative_zero_keeps_its_sign() {
        assert_eq!(bits(shortest_decimal(-0.0)), bits(-0.0));
    }

    #[test]
    fn nan_and_the_infinities_pass_through() {
        assert!(shortest_decimal(f32::NAN).is_nan());
        assert_eq!(bits(shortest_decimal(f32::INFINITY)), bits(f64::INFINITY));
        assert_eq!(bits(shortest_decimal(f32::NEG_INFINITY)), bits(f64::NEG_INFINITY));
    }
}
