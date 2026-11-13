use cassiopeia_sensorthings::{
    service_root::ServiceRoot,
    settings::collect_limits::{ObservationLimit, PageLimit},
};
use derive_more::Display;
use getset::{CopyGetters, Getters};
use url::Url;

/// A collection limit, with the value it is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
pub enum CollectLimit {
    /// `limits.pages`, the most pages one traversal follows.
    #[display("`limits.pages: {_0}`")]
    Pages(PageLimit),
    /// `limits.observations`, the most Observations one cycle writes.
    #[display("`limits.observations: {_0}`")]
    Observations(ObservationLimit),
}

/// A collection limit that stopped a read before it collected all it was asked for. The cycle
/// carries on with what the read collected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Getters, CopyGetters)]
pub struct LimitReached {
    /// The service root the read was built on.
    #[getset(get = "pub")]
    root: ServiceRoot,
    /// The limit that stopped it.
    #[getset(get_copy = "pub")]
    limit: CollectLimit,
    /// Where the read stopped: the next link a page limit kept it from following, or the page an
    /// observation limit stopped it in, which holds the first Observation it did not write.
    #[getset(get = "pub")]
    next: Url,
}

/// What a collection limit left unread, written as the clause a report of the limit ends with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum LeftUnread<'a> {
    /// The next link a page limit kept the read from following.
    #[display("the next link `{_0}` was not followed")]
    NextLink(&'a Url),
    /// The page an observation limit stopped the read in.
    #[display("`{_0}` holds the first Observation the read did not write")]
    RestOfPage(&'a Url),
}

impl LimitReached {
    /// The `limit` that stopped the read of `root` at `next`.
    #[must_use]
    pub const fn new(root: ServiceRoot, limit: CollectLimit, next: Url) -> LimitReached {
        LimitReached { root, limit, next }
    }

    /// What the limit left unread.
    #[must_use]
    pub const fn unread(&self) -> LeftUnread<'_> {
        match self.limit {
            CollectLimit::Pages(_) => LeftUnread::NextLink(&self.next),
            CollectLimit::Observations(_) => LeftUnread::RestOfPage(&self.next),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::limit_reached::{CollectLimit, LimitReached};
    use cassiopeia_sensorthings::{
        service_root::ServiceRoot,
        settings::collect_limits::{ObservationLimit, PageLimit},
    };
    use pretty_assertions::assert_eq;
    use url::Url;

    fn reached(limit: CollectLimit) -> LimitReached {
        LimitReached::new(
            ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap(),
            limit,
            Url::parse("https://h.example/v1.1/Datastreams(5)/Observations?$skip=100").unwrap(),
        )
    }

    #[test]
    fn a_limit_names_its_setting_and_value() {
        assert_eq!(CollectLimit::Pages(PageLimit::try_from(2).unwrap()).to_string(), "`limits.pages: 2`");
        assert_eq!(
            CollectLimit::Observations(ObservationLimit::try_from(5).unwrap()).to_string(),
            "`limits.observations: 5`"
        );
    }

    #[test]
    fn a_page_limit_leaves_its_next_link_unfollowed() {
        assert_eq!(
            reached(CollectLimit::Pages(PageLimit::try_from(2).unwrap())).unread().to_string(),
            "the next link `https://h.example/v1.1/Datastreams(5)/Observations?$skip=100` was not followed"
        );
    }

    #[test]
    fn an_observation_limit_leaves_the_rest_of_its_page_unwritten() {
        assert_eq!(
            reached(CollectLimit::Observations(ObservationLimit::try_from(5).unwrap())).unread().to_string(),
            "`https://h.example/v1.1/Datastreams(5)/Observations?$skip=100` holds the first Observation the read did not write"
        );
    }
}
