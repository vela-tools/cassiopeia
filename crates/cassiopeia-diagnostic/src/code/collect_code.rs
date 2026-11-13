use strum::{Display, EnumCount, EnumIter};

/// Why collecting a source from a server went other than asked, while the run carried on.
#[derive(Clone, Copy, Debug, Display, EnumCount, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "kebab-case")]
pub enum CollectCode {
    /// A collection limit stopped a read before the server's last page, so the cycle went on with
    /// what it had collected.
    LimitReached,
}

#[cfg(test)]
mod tests {
    use crate::code::collect_code::CollectCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_collect_code_renders_a_kebab_case_token() {
        assert_eq!(CollectCode::LimitReached.to_string(), "limit-reached");
    }
}
