use crate::value::non_finite_float::NonFiniteFloat;
use derive_more::{Display, Into};
use serde::{Deserialize, Serialize};

/// A float that JSON has a number for: neither `NaN` nor an infinity.
///
/// JSON has no number for a non-finite float (RFC 8259 clause 6), and a serializer writes one as
/// `null`, which cannot be used as a value anywhere in NGSI-LD (ETSI GS CIM 009 v1.9.1 clause
/// 4.5.0). Holding only a finite float makes such a value impossible to build rather than something
/// every writer has to refuse.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Display, Into, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct FiniteFloat(f64);

impl FiniteFloat {
    /// The float this holds, which is always finite.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

// Manual because the conversion validates its input, which no derive expresses.
impl TryFrom<f64> for FiniteFloat {
    type Error = NonFiniteFloat;

    fn try_from(float: f64) -> Result<FiniteFloat, NonFiniteFloat> {
        if float.is_finite() {
            Ok(FiniteFloat(float))
        } else {
            Err(NonFiniteFloat(float))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value::{finite_float::FiniteFloat, non_finite_float::NonFiniteFloat};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_finite_float_is_held_as_it_is() {
        assert_eq!(FiniteFloat::try_from(-21.5).map(FiniteFloat::get), Ok(-21.5));
        assert_eq!(FiniteFloat::try_from(0.0).map(FiniteFloat::get), Ok(0.0));
    }

    #[test]
    fn an_infinity_is_refused() {
        assert_eq!(FiniteFloat::try_from(f64::INFINITY), Err(NonFiniteFloat(f64::INFINITY)));
        assert_eq!(FiniteFloat::try_from(f64::NEG_INFINITY), Err(NonFiniteFloat(f64::NEG_INFINITY)));
    }

    #[test]
    fn nan_is_refused() {
        assert!(FiniteFloat::try_from(f64::NAN).is_err());
    }

    #[test]
    fn a_finite_float_serializes_as_a_json_number() {
        assert_eq!(serde_json::to_string(&FiniteFloat::try_from(2.5).unwrap()).unwrap(), "2.5");
    }

    #[test]
    fn deserializing_reads_a_json_number_as_a_finite_float() {
        assert_eq!(serde_json::from_str::<FiniteFloat>("2.5").unwrap(), FiniteFloat::try_from(2.5).unwrap());
    }

    #[test]
    fn the_float_displays_as_the_number_it_holds() {
        assert_eq!(FiniteFloat::try_from(2.5).unwrap().to_string(), "2.5");
    }
}
