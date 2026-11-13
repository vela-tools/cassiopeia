use chrono::{DateTime, Utc};
use getset::CopyGetters;

/// The span of `phenomenonTime` a range read requests: from `from`, included, to `to`, excluded.
///
/// `from` is always before `to`, so a window holds at least one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, CopyGetters)]
#[getset(get_copy = "pub")]
pub struct ReadWindow {
    /// The earliest `phenomenonTime` read.
    from: DateTime<Utc>,
    /// The first `phenomenonTime` past the window.
    to: DateTime<Utc>,
}

/// The bounds of a range whose `from` is not before its `to`, so it holds no reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, CopyGetters)]
#[getset(get_copy = "pub")]
pub struct EmptyWindow {
    /// The start of the range.
    from: DateTime<Utc>,
    /// The end of the range, which is not after its start.
    to: DateTime<Utc>,
}

impl ReadWindow {
    /// The window from `from` to `to`.
    ///
    /// # Errors
    ///
    /// Returns the [`EmptyWindow`] of the bounds when `from` is not before `to`.
    pub fn new(from: DateTime<Utc>, to: DateTime<Utc>) -> Result<ReadWindow, EmptyWindow> {
        if from < to {
            Ok(ReadWindow { from, to })
        } else {
            Err(EmptyWindow { from, to })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::read_window::ReadWindow;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    #[test]
    fn a_window_keeps_its_bounds() {
        let window = ReadWindow::new(instant("2026-11-11T10:00:00Z"), instant("2026-11-12T10:00:00Z")).unwrap();

        assert_eq!(window.from(), instant("2026-11-11T10:00:00Z"));
        assert_eq!(window.to(), instant("2026-11-12T10:00:00Z"));
    }

    #[test]
    fn bounds_that_are_not_in_order_are_an_empty_window() {
        for (from, to) in [
            ("2026-11-13T00:00:00Z", "2026-11-12T10:00:00Z"),
            ("2026-11-12T10:00:00Z", "2026-11-12T10:00:00Z"),
        ] {
            let empty = ReadWindow::new(instant(from), instant(to)).unwrap_err();

            assert_eq!((empty.from(), empty.to()), (instant(from), instant(to)));
        }
    }
}
