use strum::{Display, EnumCount, EnumIter};

/// Why a resolved entity could not be built into its NGSI-LD form.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum TransformCode {
    /// The NGSI-LD entity could not be assembled from its builder.
    EntityUnbuildable,
    /// An attribute's `observedAt` carried text that reads as no supported spelling of a date-time,
    /// so the attribute was emitted without the qualifier (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2).
    ObservedAtUnreadable,
}

#[cfg(test)]
mod tests {
    use crate::code::transform_code::TransformCode;

    #[test]
    fn a_transform_code_renders_a_kebab_case_token() {
        assert_eq!(TransformCode::EntityUnbuildable.to_string(), "entity-unbuildable");
        assert_eq!(TransformCode::ObservedAtUnreadable.to_string(), "observed-at-unreadable");
    }
}
