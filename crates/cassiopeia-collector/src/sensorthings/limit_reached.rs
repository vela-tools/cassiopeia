use cassiopeia_sensorthings::{service_root::ServiceRoot, settings::collect_limits::PageLimit};
use derive_more::Display;
use getset::{CopyGetters, Getters};
use url::Url;

/// A collection limit, with the value it is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
pub enum CollectLimit {
    /// `limits.pages`, the most pages one traversal follows.
    #[display("`limits.pages: {_0}`")]
    Pages(PageLimit),
}

/// A collection limit that stopped a read before the server's last page. The cycle carries on with
/// what the read collected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Getters, CopyGetters)]
pub struct LimitReached {
    /// The service root the read was built on.
    #[getset(get = "pub")]
    root: ServiceRoot,
    /// The limit that stopped it.
    #[getset(get_copy = "pub")]
    limit: CollectLimit,
    /// The next link the read did not follow.
    #[getset(get = "pub")]
    next: Url,
}

impl LimitReached {
    /// The `limit` that stopped the read of `root` before the next link `next`.
    #[must_use]
    pub const fn new(root: ServiceRoot, limit: CollectLimit, next: Url) -> LimitReached {
        LimitReached { root, limit, next }
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::limit_reached::CollectLimit;
    use cassiopeia_sensorthings::settings::collect_limits::PageLimit;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_limit_names_its_setting_and_value() {
        assert_eq!(CollectLimit::Pages(PageLimit::try_from(2).unwrap()).to_string(), "`limits.pages: 2`");
    }
}
