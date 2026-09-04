use crate::strategy::{Altitude, ConversionStrategy, Winding};
use getset::Getters;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// How a mapping wants one attribute's geometry handled.
///
/// The `transformation` names the geometry type to produce; this says what may be lost getting
/// there. Its absence is the strict default: identity, promotion to a multi-geometry, and
/// unwrapping a multi-geometry of exactly one member all pass, and every conversion that would
/// discard coordinates is refused until the mapping names which loss it accepts.
///
/// `deny_unknown_fields` is what keeps a typo'd sub-key from being read as "no policy at all":
/// an attribute declaration itself accepts unknown keys, so a misspelled `convert` would otherwise
/// be dropped in silence.
#[derive(Clone, Copy, Debug, Default, Deserialize, Getters, PartialEq, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeometryPolicy {
    /// The named conversion applied before the target type is reconciled, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    #[getset(get = "pub")]
    convert: Option<ConversionStrategy>,

    /// Whether polygon rings are rewound to RFC 7946 clause 3.1.6's right-hand rule.
    #[serde(default)]
    #[builder(default = Default::default())]
    #[getset(get = "pub")]
    winding: Winding,

    /// Whether a position's optional altitude survives.
    #[serde(default)]
    #[builder(default = Default::default())]
    #[getset(get = "pub")]
    altitude: Altitude,
}

#[cfg(test)]
mod tests {
    use crate::{
        policy::GeometryPolicy,
        strategy::{Altitude, ConversionStrategy, Winding},
    };

    #[test]
    fn an_absent_policy_normalises_winding_keeps_altitude_and_declares_no_conversion() {
        let policy = GeometryPolicy::default();

        assert_eq!(policy.convert(), &None);
        assert_eq!(policy.winding(), &Winding::Rfc7946);
        assert_eq!(policy.altitude(), &Altitude::Keep);
    }

    #[test]
    fn a_policy_reads_its_three_keys() {
        let policy: GeometryPolicy = serde_json::from_str(r#"{"convert": "largest", "winding": "keep", "altitude": "drop"}"#).unwrap();

        assert_eq!(policy.convert(), &Some(ConversionStrategy::Largest));
        assert_eq!(policy.winding(), &Winding::Keep);
        assert_eq!(policy.altitude(), &Altitude::Drop);
    }

    #[test]
    fn a_mistyped_sub_key_is_rejected_rather_than_ignored() {
        assert!(serde_json::from_str::<GeometryPolicy>(r#"{"converts": "largest"}"#).is_err());
    }
}
