use strum::{Display, EnumCount, EnumIter};

/// Why collecting a source from a server went other than asked, while the run carried on.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum CollectCode {
    /// A collection limit stopped a read before it collected all it was asked for, so the cycle went
    /// on with what it had collected.
    LimitReached,
    /// A range read's range held no reading in its cycle, so the read requested nothing.
    RangeEmpty,
    /// A server no longer lists data streams an incremental read's cursor keeps a position for, so
    /// the cycle read nothing of them and kept their positions.
    DatastreamsMissing,
}

#[cfg(test)]
mod tests {
    use crate::code::collect_code::CollectCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_collect_code_renders_a_kebab_case_token() {
        assert_eq!(CollectCode::LimitReached.to_string(), "limit-reached");
        assert_eq!(CollectCode::RangeEmpty.to_string(), "range-empty");
        assert_eq!(CollectCode::DatastreamsMissing.to_string(), "datastreams-missing");
    }
}
