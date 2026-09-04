use strum::{Display, EnumCount, EnumIter};

/// Why a source record could not be expanded into fragments.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum ExpanderCode {
    /// The entity URN or scope a record maps to could not be generated.
    UrnUngeneratable,
    /// The record's source collection is bound to no mapping.
    CollectionUnmatched,
    /// The record carries no source collection, which a collections binding requires.
    CollectionMissing,
}

#[cfg(test)]
mod tests {
    use crate::code::expander_code::ExpanderCode;

    #[test]
    fn an_expander_code_renders_a_kebab_case_token() {
        assert_eq!(ExpanderCode::CollectionUnmatched.to_string(), "collection-unmatched");
    }
}
