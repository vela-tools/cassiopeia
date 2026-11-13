use cassiopeia_sensorthings::{read_window::EmptyWindow, service_root::ServiceRoot};
use getset::{CopyGetters, Getters};

/// A range read whose range held no reading in its cycle, because its start was not before its end,
/// so the read requested nothing and its snapshot holds its header alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Getters, CopyGetters)]
pub struct RangeEmpty {
    /// The service root the read was built on.
    #[getset(get = "pub")]
    root: ServiceRoot,
    /// The bounds of the range in that cycle.
    #[getset(get_copy = "pub")]
    window: EmptyWindow,
}

impl RangeEmpty {
    /// The empty range `window` of a read of `root`.
    #[must_use]
    pub const fn new(root: ServiceRoot, window: EmptyWindow) -> RangeEmpty {
        RangeEmpty { root, window }
    }
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::range_empty::RangeEmpty;
    use cassiopeia_sensorthings::{read_window::ReadWindow, service_root::ServiceRoot};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use url::Url;

    #[test]
    fn an_empty_range_keeps_its_root_and_bounds() {
        let instant = |text: &str| -> DateTime<Utc> { DateTime::parse_from_rfc3339(text).unwrap().to_utc() };
        let root = ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap();
        let window = ReadWindow::new(instant("2026-11-13T00:00:00Z"), instant("2026-11-12T10:00:00Z")).unwrap_err();

        let empty = RangeEmpty::new(root.clone(), window);

        assert_eq!(empty.root(), &root);
        assert_eq!(empty.window(), window);
    }
}
