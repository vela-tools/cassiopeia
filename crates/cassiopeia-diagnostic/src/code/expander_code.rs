use strum::{Display, EnumCount, EnumIter};

/// Why a source record could not be expanded into fragments.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum ExpanderCode {
    /// The entity URN or scope a record maps to could not be generated.
    UrnUngeneratable,
    /// The entity `expiresAt` a record resolved to reads as no instant, so the entity publishes
    /// without the member (ETSI GS CIM 009 v1.9.1 clause 4.5.1).
    ExpiryUnreadable,
    /// The entity `expiresAt` template failed to render for a record, so the record was skipped.
    ExpiryUnresolvable,
    /// The record's source collection is bound to no mapping.
    CollectionUnmatched,
    /// The record carries no source collection, which a collections binding requires.
    CollectionMissing,
}

#[cfg(test)]
mod tests {
    use crate::code::expander_code::ExpanderCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_expander_code_renders_a_kebab_case_token() {
        assert_eq!(ExpanderCode::CollectionUnmatched.to_string(), "collection-unmatched");
        assert_eq!(ExpanderCode::ExpiryUnreadable.to_string(), "expiry-unreadable");
        assert_eq!(ExpanderCode::ExpiryUnresolvable.to_string(), "expiry-unresolvable");
    }
}
