use thiserror::Error;

/// A float that is `NaN` or an infinity, which no NGSI-LD Value can hold.
///
/// JSON has no number for it (RFC 8259 clause 6), and the `null` a serializer writes in its place
/// cannot be used as a value anywhere in NGSI-LD (ETSI GS CIM 009 v1.9.1 clause 4.5.0).
#[derive(Debug, Clone, Copy, Error, PartialEq)]
#[error("{0} is not a finite number, and JSON has no number for it")]
pub struct NonFiniteFloat(pub f64);

#[cfg(test)]
mod tests {
    use crate::value::non_finite_float::NonFiniteFloat;

    #[test]
    fn the_message_names_the_float_refused() {
        let message = NonFiniteFloat(f64::INFINITY).to_string();

        assert!(message.contains("inf"), "{message}");
    }
}
