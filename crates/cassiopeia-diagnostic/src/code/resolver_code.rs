use strum::{Display, EnumCount, EnumIter};

/// Why the resolver, which merges every record's fragments into entities, has something to report.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum ResolverCode {
    /// Several records of one mapping resolved to the same entity id and disagreed on a field, so
    /// merging them into one entity kept one value and discarded the others.
    RecordsMerged,
}

#[cfg(test)]
mod tests {
    use crate::code::resolver_code::ResolverCode;

    #[test]
    fn a_resolver_code_renders_a_kebab_case_token() {
        assert_eq!(ResolverCode::RecordsMerged.to_string(), "records-merged");
    }
}
