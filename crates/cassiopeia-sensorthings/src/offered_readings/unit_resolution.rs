use cefact_units::UnitCode;
use derive_more::Display;

/// What the `cefact_unit` filter makes of a data stream's unit: the UN/CEFACT Recommendation No. 20
/// code the unit resolves to, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display)]
pub enum UnitResolution {
    /// The unit resolves to this code, which the filter writes in its place.
    #[display("{}", _0.as_str())]
    Code(UnitCode),

    /// The unit resolves to no code, so the filter passes its text through unchanged.
    #[display("unknown")]
    Unknown,
}

#[cfg(test)]
mod tests {
    use crate::offered_readings::unit_resolution::UnitResolution;
    use cefact_units::UnitCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_resolution_is_displayed_as_its_code_or_as_unknown() {
        assert_eq!(UnitResolution::Code(UnitCode::Gq).to_string(), "GQ");
        assert_eq!(UnitResolution::Unknown.to_string(), "unknown");
    }
}
